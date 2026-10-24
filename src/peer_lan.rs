use crate::lan_access_control::LanEnableControl;
use crate::network_safety;
use crate::peer_identity::{self, PEER_FINGERPRINT_BYTES};
use crate::peer_transfer::{self, ReceivedTransfer, TransferPayload};
use crate::received_file_integrity::{ReceivedFileIntegrityError, VerifiedReceivedFile};
use crate::trusted_peers;
use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

const HELLO_MAGIC: &[u8; 8] = b"DGMLANH1";
const OFFER_MAGIC: &[u8; 8] = b"DGMLOFR1";
const ENVELOPE_MAGIC: &[u8; 8] = b"DGMLENV1";
const ACK_MAGIC: &[u8; 8] = b"DGMLACK1";
const LAN_PROTOCOL_VERSION: u16 = 1;
const HELLO_BYTES: usize = 8 + 2 + PEER_FINGERPRINT_BYTES;
const FRAME_HEADER_BYTES: usize = 8 + 4;
const ACK_BYTES: usize = 8 + 1;
const ACK_ACCEPTED: u8 = 1;

pub const DEFAULT_PEER_LAN_PORT: u16 = 45_822;
pub const MAX_RECEIVE_WAIT: Duration = Duration::from_secs(5 * 60);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub enum LanReceivedTransfer {
    Text {
        sender_fingerprint: [u8; PEER_FINGERPRINT_BYTES],
        text: String,
    },
    File(VerifiedReceivedFile),
}

impl fmt::Debug for LanReceivedTransfer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text {
                sender_fingerprint,
                text,
            } => formatter
                .debug_struct("LanReceivedTransfer::Text")
                .field("sender_fingerprint", &hex_upper(sender_fingerprint))
                .field("bytes", &text.len())
                .finish(),
            Self::File(file) => formatter
                .debug_tuple("LanReceivedTransfer::File")
                .field(file)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerLanError {
    LanIntentOff,
    NetworkExposureBlocked,
    UnsupportedAddress,
    ReceiveWindowTooLong,
    BindFailed,
    ConnectFailed,
    Io,
    InvalidProtocol,
    PeerStateUnavailable,
    PeerNotTrusted,
    TransferRejected,
    FileIntegrityRejected,
    RemoteRejected,
}

impl fmt::Display for PeerLanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::LanIntentOff => "direct LAN requires explicit LAN enablement for this run",
            Self::NetworkExposureBlocked => {
                "direct LAN is blocked by the current network-safety assessment"
            }
            Self::UnsupportedAddress => {
                "direct LAN accepts only IPv4 loopback, private, or link-local addresses"
            }
            Self::ReceiveWindowTooLong => "direct LAN receive window exceeds five minutes",
            Self::BindFailed => "direct LAN listener could not bind",
            Self::ConnectFailed => "direct LAN peer connection failed",
            Self::Io => "direct LAN I/O failed",
            Self::InvalidProtocol => "direct LAN framing is invalid",
            Self::PeerStateUnavailable => "trusted-peer state is unavailable",
            Self::PeerNotTrusted => "remote peer is not currently trusted",
            Self::TransferRejected => "encrypted peer transfer was rejected",
            Self::FileIntegrityRejected => "received file integrity verification failed",
            Self::RemoteRejected => "remote peer did not acknowledge the transfer",
        };
        formatter.write_str(message)
    }
}

impl Error for PeerLanError {}

pub fn receive_one(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    port: u16,
    wait: Duration,
) -> Result<Option<LanReceivedTransfer>, PeerLanError> {
    receive_one_with_gate(workspace_root, lan_control, port, wait, &CurrentExposureGate)
}

pub fn send_transfer(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    target: SocketAddr,
    expected_recipient_fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
    payload: TransferPayload,
) -> Result<(), PeerLanError> {
    send_transfer_with_gate(
        workspace_root,
        lan_control,
        target,
        expected_recipient_fingerprint,
        payload,
        &CurrentExposureGate,
    )
}

trait ExposureGate {
    fn exposure_allowed(&self, lan_control: &LanEnableControl) -> bool;
}

struct CurrentExposureGate;

impl ExposureGate for CurrentExposureGate {
    fn exposure_allowed(&self, lan_control: &LanEnableControl) -> bool {
        network_safety::current_exposure_decision(lan_control)
            .1
            .allows_exposure()
    }
}

fn require_fresh_exposure<G: ExposureGate>(
    lan_control: &LanEnableControl,
    gate: &G,
) -> Result<(), PeerLanError> {
    if !lan_control.is_enabled() {
        return Err(PeerLanError::LanIntentOff);
    }
    if !gate.exposure_allowed(lan_control) {
        return Err(PeerLanError::NetworkExposureBlocked);
    }
    Ok(())
}

fn receive_one_with_gate<G: ExposureGate>(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    port: u16,
    wait: Duration,
    gate: &G,
) -> Result<Option<LanReceivedTransfer>, PeerLanError> {
    if wait > MAX_RECEIVE_WAIT {
        return Err(PeerLanError::ReceiveWindowTooLong);
    }

    require_fresh_exposure(lan_control, gate)?;

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .map_err(|_| PeerLanError::BindFailed)?;
    receive_one_from_listener_with_gate(workspace_root, lan_control, listener, wait, gate)
}

fn receive_one_from_listener_with_gate<G: ExposureGate>(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    listener: TcpListener,
    wait: Duration,
    gate: &G,
) -> Result<Option<LanReceivedTransfer>, PeerLanError> {
    listener
        .set_nonblocking(true)
        .map_err(|_| PeerLanError::Io)?;

    let deadline = Instant::now()
        .checked_add(wait)
        .ok_or(PeerLanError::ReceiveWindowTooLong)?;

    loop {
        require_fresh_exposure(lan_control, gate)?;

        match listener.accept() {
            Ok((stream, remote)) => {
                drop(listener);
                require_fresh_exposure(lan_control, gate)?;
                if !is_direct_lan_address(remote) {
                    return Err(PeerLanError::UnsupportedAddress);
                }
                return handle_inbound_stream(
                    workspace_root,
                    lan_control,
                    gate,
                    stream,
                )
                .map(Some);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => return Err(PeerLanError::Io),
        }
    }
}

fn send_transfer_with_gate<G: ExposureGate>(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    target: SocketAddr,
    expected_recipient_fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
    payload: TransferPayload,
    gate: &G,
) -> Result<(), PeerLanError> {
    if !is_direct_lan_address(target) {
        return Err(PeerLanError::UnsupportedAddress);
    }

    require_fresh_exposure(lan_control, gate)?;

    let trusted = trusted_peers::load(workspace_root)
        .map_err(|_| PeerLanError::PeerStateUnavailable)?;
    if !trusted.contains(expected_recipient_fingerprint) {
        return Err(PeerLanError::PeerNotTrusted);
    }

    let local = peer_identity::load(workspace_root)
        .map_err(|_| PeerLanError::PeerStateUnavailable)?
        .ok_or(PeerLanError::PeerStateUnavailable)?;

    let mut stream =
        TcpStream::connect_timeout(&target, CONNECT_TIMEOUT).map_err(|_| PeerLanError::ConnectFailed)?;
    configure_stream(&stream)?;
    require_fresh_exposure(lan_control, gate)?;

    write_hello(&mut stream, local.fingerprint())?;
    let offer = read_frame(
        &mut stream,
        OFFER_MAGIC,
        peer_transfer::PEER_TRANSFER_OFFER_BYTES,
    )?;
    if offer.len() != peer_transfer::PEER_TRANSFER_OFFER_BYTES {
        return Err(PeerLanError::InvalidProtocol);
    }

    require_fresh_exposure(lan_control, gate)?;
    let envelope = peer_transfer::seal_transfer_to(
        workspace_root,
        &offer,
        expected_recipient_fingerprint,
        payload,
    )
    .map_err(|_| PeerLanError::TransferRejected)?;

    write_frame(&mut stream, ENVELOPE_MAGIC, &envelope)?;
    read_ack(&mut stream)?;
    Ok(())
}

fn handle_inbound_stream<G: ExposureGate>(
    workspace_root: &Path,
    lan_control: &LanEnableControl,
    gate: &G,
    mut stream: TcpStream,
) -> Result<LanReceivedTransfer, PeerLanError> {
    configure_stream(&stream)?;
    require_fresh_exposure(lan_control, gate)?;

    let sender_fingerprint = read_hello(&mut stream)?;
    let trusted = trusted_peers::load(workspace_root)
        .map_err(|_| PeerLanError::PeerStateUnavailable)?;
    if !trusted.contains(&sender_fingerprint) {
        return Err(PeerLanError::PeerNotTrusted);
    }

    let pending = peer_transfer::begin_receive(workspace_root, &sender_fingerprint, Instant::now())
        .map_err(|_| PeerLanError::TransferRejected)?;
    let offer = pending.offer_bytes();
    write_frame(&mut stream, OFFER_MAGIC, &offer)?;

    let envelope = read_frame(
        &mut stream,
        ENVELOPE_MAGIC,
        peer_transfer::MAX_ENCRYPTED_TRANSFER_BYTES,
    )?;

    require_fresh_exposure(lan_control, gate)?;
    let received = peer_transfer::open_transfer(
        workspace_root,
        pending,
        &envelope,
        Instant::now(),
    )
    .map_err(|_| PeerLanError::TransferRejected)?;

    let accepted = match received {
        ReceivedTransfer::Text(text) => LanReceivedTransfer::Text {
            sender_fingerprint,
            text,
        },
        ReceivedTransfer::File(pending_file) => {
            let verified = pending_file
                .verify()
                .map_err(map_integrity_error)?;
            LanReceivedTransfer::File(verified)
        }
    };

    write_ack(&mut stream)?;
    Ok(accepted)
}

fn map_integrity_error(_error: ReceivedFileIntegrityError) -> PeerLanError {
    PeerLanError::FileIntegrityRejected
}

fn configure_stream(stream: &TcpStream) -> Result<(), PeerLanError> {
    stream
        .set_nonblocking(false)
        .map_err(|_| PeerLanError::Io)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|_| PeerLanError::Io)?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|_| PeerLanError::Io)?;
    stream.set_nodelay(true).map_err(|_| PeerLanError::Io)?;
    Ok(())
}

fn write_hello(
    stream: &mut TcpStream,
    sender_fingerprint: &[u8; PEER_FINGERPRINT_BYTES],
) -> Result<(), PeerLanError> {
    let mut hello = Vec::with_capacity(HELLO_BYTES);
    hello.extend_from_slice(HELLO_MAGIC);
    hello.extend_from_slice(&LAN_PROTOCOL_VERSION.to_be_bytes());
    hello.extend_from_slice(sender_fingerprint);
    stream.write_all(&hello).map_err(|_| PeerLanError::Io)
}

fn read_hello(
    stream: &mut TcpStream,
) -> Result<[u8; PEER_FINGERPRINT_BYTES], PeerLanError> {
    let mut hello = [0_u8; HELLO_BYTES];
    stream
        .read_exact(&mut hello)
        .map_err(|_| PeerLanError::InvalidProtocol)?;

    if &hello[..8] != HELLO_MAGIC
        || u16::from_be_bytes([hello[8], hello[9]]) != LAN_PROTOCOL_VERSION
    {
        return Err(PeerLanError::InvalidProtocol);
    }

    hello[10..]
        .try_into()
        .map_err(|_| PeerLanError::InvalidProtocol)
}

fn write_frame(
    stream: &mut TcpStream,
    magic: &[u8; 8],
    payload: &[u8],
) -> Result<(), PeerLanError> {
    let len = u32::try_from(payload.len()).map_err(|_| PeerLanError::InvalidProtocol)?;
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    header[..8].copy_from_slice(magic);
    header[8..].copy_from_slice(&len.to_be_bytes());
    stream.write_all(&header).map_err(|_| PeerLanError::Io)?;
    stream.write_all(payload).map_err(|_| PeerLanError::Io)
}

fn read_frame(
    stream: &mut TcpStream,
    magic: &[u8; 8],
    max_payload: usize,
) -> Result<Vec<u8>, PeerLanError> {
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    stream
        .read_exact(&mut header)
        .map_err(|_| PeerLanError::InvalidProtocol)?;
    if &header[..8] != magic {
        return Err(PeerLanError::InvalidProtocol);
    }

    let declared = u32::from_be_bytes(
        header[8..12]
            .try_into()
            .map_err(|_| PeerLanError::InvalidProtocol)?,
    ) as usize;
    if declared == 0 || declared > max_payload {
        return Err(PeerLanError::InvalidProtocol);
    }

    let mut payload = vec![0_u8; declared];
    stream
        .read_exact(&mut payload)
        .map_err(|_| PeerLanError::InvalidProtocol)?;
    Ok(payload)
}

fn write_ack(stream: &mut TcpStream) -> Result<(), PeerLanError> {
    let mut ack = [0_u8; ACK_BYTES];
    ack[..8].copy_from_slice(ACK_MAGIC);
    ack[8] = ACK_ACCEPTED;
    stream.write_all(&ack).map_err(|_| PeerLanError::Io)
}

fn read_ack(stream: &mut TcpStream) -> Result<(), PeerLanError> {
    let mut ack = [0_u8; ACK_BYTES];
    stream
        .read_exact(&mut ack)
        .map_err(|_| PeerLanError::RemoteRejected)?;
    if &ack[..8] != ACK_MAGIC || ack[8] != ACK_ACCEPTED {
        return Err(PeerLanError::RemoteRejected);
    }
    Ok(())
}

fn is_direct_lan_address(address: SocketAddr) -> bool {
    match address {
        SocketAddr::V4(address) => is_direct_lan_ipv4(*address.ip()),
        SocketAddr::V6(_) => false,
    }
}

fn is_direct_lan_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    address.is_loopback()
        || octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
        || (octets[0] == 169 && octets[1] == 254)
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[(byte >> 4) as usize]));
        output.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_identity::{
        PeerRandomSource, PeerRandomSourceError, PeerVerificationSubmission,
        PEER_VERIFICATION_CHALLENGE_BYTES,
    };
    use crate::trusted_peers;
    use crate::workspace;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct AlwaysTrustedGate;

    impl ExposureGate for AlwaysTrustedGate {
        fn exposure_allowed(&self, _lan_control: &LanEnableControl) -> bool {
            true
        }
    }

    struct FixedRandom([u8; PEER_VERIFICATION_CHALLENGE_BYTES]);

    impl PeerRandomSource for FixedRandom {
        fn fill_bytes(
            &mut self,
            destination: &mut [u8],
        ) -> Result<(), PeerRandomSourceError> {
            if destination.len() > self.0.len() {
                return Err(PeerRandomSourceError);
            }
            destination.copy_from_slice(&self.0[..destination.len()]);
            Ok(())
        }
    }

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dangomushi-peer-lan-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn initialize_identity(
        root: &Path,
        seed: u8,
    ) -> crate::peer_identity::LocalPeerIdentity {
        let mut random = FixedRandom([seed; PEER_VERIFICATION_CHALLENGE_BYTES]);
        crate::peer_identity::initialize_with_source(root, &mut random)
            .expect("initialize identity")
    }

    fn verified_candidate(
        peer: &crate::peer_identity::LocalPeerIdentity,
        peer_name: &str,
        challenge_byte: u8,
    ) -> crate::peer_identity::VerifiedPeerCandidate {
        let now = Instant::now();
        let mut random = FixedRandom([challenge_byte; PEER_VERIFICATION_CHALLENGE_BYTES]);
        let challenge =
            crate::peer_identity::begin_verification_with_source(now, &mut random)
                .expect("challenge");
        let signature = peer
            .sign_verification(challenge.bytes(), peer_name)
            .expect("sign");

        crate::peer_identity::complete_verification(
            now + Duration::from_secs(1),
            challenge,
            PeerVerificationSubmission {
                peer_name: peer_name.to_string(),
                public_key: *peer.public_key(),
                signature,
            },
        )
        .expect("candidate")
    }

    struct Pair {
        sender_root: PathBuf,
        receiver_root: PathBuf,
        sender: crate::peer_identity::LocalPeerIdentity,
        receiver: crate::peer_identity::LocalPeerIdentity,
    }

    impl Pair {
        fn new(label: &str) -> Self {
            let sender_root = temporary_workspace(&format!("{label}-sender"));
            let receiver_root = temporary_workspace(&format!("{label}-receiver"));
            workspace::initialize(&sender_root).expect("sender workspace");
            workspace::initialize(&receiver_root).expect("receiver workspace");

            let sender = initialize_identity(&sender_root, 81);
            let receiver = initialize_identity(&receiver_root, 82);

            let receiver_candidate = verified_candidate(&receiver, "Receiver", 91);
            trusted_peers::trust_verified_peer(&sender_root, &receiver_candidate)
                .expect("sender trusts receiver");
            let sender_candidate = verified_candidate(&sender, "Sender", 92);
            trusted_peers::trust_verified_peer(&receiver_root, &sender_candidate)
                .expect("receiver trusts sender");

            Self {
                sender_root,
                receiver_root,
                sender,
                receiver,
            }
        }
    }

    impl Drop for Pair {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.sender_root);
            let _ = fs::remove_dir_all(&self.receiver_root);
        }
    }

    fn enabled_lan() -> LanEnableControl {
        let mut lan = LanEnableControl::default();
        lan.enable_for_this_run();
        lan
    }

    fn bound_test_listener() -> (TcpListener, u16) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind test listener");
        let port = listener.local_addr().expect("local address").port();
        (listener, port)
    }

    #[test]
    fn direct_lan_address_policy_rejects_global_addresses() {
        assert!(is_direct_lan_ipv4(Ipv4Addr::LOCALHOST));
        assert!(is_direct_lan_ipv4(Ipv4Addr::new(10, 1, 2, 3)));
        assert!(is_direct_lan_ipv4(Ipv4Addr::new(172, 20, 1, 1)));
        assert!(is_direct_lan_ipv4(Ipv4Addr::new(192, 168, 1, 5)));
        assert!(is_direct_lan_ipv4(Ipv4Addr::new(169, 254, 2, 2)));
        assert!(!is_direct_lan_ipv4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_direct_lan_ipv4(Ipv4Addr::new(100, 64, 0, 1)));
    }

    #[test]
    fn explicit_lan_intent_is_still_required_even_with_trusted_test_gate() {
        let root = temporary_workspace("intent");
        workspace::initialize(&root).expect("workspace");
        let lan = LanEnableControl::default();
        assert_eq!(
            receive_one_with_gate(
                &root,
                &lan,
                0,
                Duration::from_millis(1),
                &AlwaysTrustedGate,
            )
            .unwrap_err(),
            PeerLanError::LanIntentOff
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn actual_tcp_text_transfer_round_trips_through_existing_crypto() {
        let pair = Pair::new("text");
        let (listener, port) = bound_test_listener();
        let lan = enabled_lan();
        let receiver_root = pair.receiver_root.clone();
        let sender_fingerprint = *pair.sender.fingerprint();
        let (tx, rx) = mpsc::channel();

        let server = thread::spawn(move || {
            let result = receive_one_from_listener_with_gate(
                &receiver_root,
                &lan,
                listener,
                Duration::from_secs(5),
                &AlwaysTrustedGate,
            );
            tx.send(result).expect("send result");
        });

        thread::sleep(Duration::from_millis(25));
        let send_result = send_transfer_with_gate(
            &pair.sender_root,
            &enabled_lan(),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
            pair.receiver.fingerprint(),
            TransferPayload::Text("LAN encrypted text".to_string()),
            &AlwaysTrustedGate,
        );
        let receive_result = rx.recv_timeout(Duration::from_secs(5)).expect("receive result");
        assert!(
            send_result.is_ok(),
            "send failed: {send_result:?}; receiver returned: {receive_result:?}"
        );

        match receive_result.expect("receive") {
            Some(LanReceivedTransfer::Text {
                sender_fingerprint: actual,
                text,
            }) => {
                assert_eq!(actual, sender_fingerprint);
                assert_eq!(text, "LAN encrypted text");
            }
            other => panic!("unexpected receive result: {other:?}"),
        }

        server.join().expect("server");
    }

    #[test]
    fn actual_tcp_file_transfer_returns_only_verified_file() {
        let pair = Pair::new("file");
        let (listener, port) = bound_test_listener();
        let lan = enabled_lan();
        let receiver_root = pair.receiver_root.clone();
        let (tx, rx) = mpsc::channel();

        let server = thread::spawn(move || {
            tx.send(receive_one_from_listener_with_gate(
                &receiver_root,
                &lan,
                listener,
                Duration::from_secs(5),
                &AlwaysTrustedGate,
            ))
            .expect("send result");
        });

        thread::sleep(Duration::from_millis(25));
        let bytes = b"verified over direct LAN".to_vec();
        let send_result = send_transfer_with_gate(
            &pair.sender_root,
            &enabled_lan(),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)),
            pair.receiver.fingerprint(),
            TransferPayload::File {
                suggested_name: "verified.bin".to_string(),
                bytes: bytes.clone(),
            },
            &AlwaysTrustedGate,
        );
        let receive_result = rx.recv_timeout(Duration::from_secs(5)).expect("receive result");
        assert!(
            send_result.is_ok(),
            "send failed: {send_result:?}; receiver returned: {receive_result:?}"
        );

        match receive_result.expect("receive") {
            Some(LanReceivedTransfer::File(file)) => {
                assert_eq!(file.suggested_name(), "verified.bin");
                assert_eq!(file.bytes(), bytes.as_slice());
                assert_eq!(file.sender_fingerprint(), pair.sender.fingerprint());
            }
            other => panic!("unexpected receive result: {other:?}"),
        }

        server.join().expect("server");
    }

    #[test]
    fn receive_window_is_bounded() {
        let root = temporary_workspace("window");
        workspace::initialize(&root).expect("workspace");
        assert_eq!(
            receive_one_with_gate(
                &root,
                &enabled_lan(),
                0,
                MAX_RECEIVE_WAIT + Duration::from_secs(1),
                &AlwaysTrustedGate,
            )
            .unwrap_err(),
            PeerLanError::ReceiveWindowTooLong
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
