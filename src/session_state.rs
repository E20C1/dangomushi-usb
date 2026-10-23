use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

const MAGIC: &str = "DANGOMUSHI_SESSION_V1";
const SESSION_FILE: &str = "session.v1";
const SESSION_PREVIOUS: &str = "session.v1.previous";
const MAX_SESSION_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionView {
    Home,
    Tools,
    Files,
    Editor,
    Workspace,
    Mobile,
    System,
}

impl SessionView {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Tools => "tools",
            Self::Files => "files",
            Self::Editor => "editor",
            Self::Workspace => "workspace",
            Self::Mobile => "mobile",
            Self::System => "system",
        }
    }

    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "home" => Ok(Self::Home),
            "tools" => Ok(Self::Tools),
            "files" => Ok(Self::Files),
            "editor" => Ok(Self::Editor),
            "workspace" => Ok(Self::Workspace),
            "mobile" => Ok(Self::Mobile),
            "system" => Ok(Self::System),
            _ => Err(invalid_data("unknown session view")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    pub view: SessionView,
    pub browser_dir: PathBuf,
    pub browser_selected: Option<PathBuf>,
    pub editor_path: Option<PathBuf>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            view: SessionView::Home,
            browser_dir: PathBuf::new(),
            browser_selected: None,
            editor_path: None,
        }
    }
}

pub fn load(root: &Path) -> io::Result<Option<SessionState>> {
    if !root.is_dir() {
        return Ok(None);
    }

    let state_dir = checked_state_dir(root, false)?;
    let path = state_dir.join(SESSION_FILE);

    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };

    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session file must be a regular non-symlink file",
        ));
    }

    if metadata.len() > MAX_SESSION_BYTES {
        return Err(invalid_data("session file exceeds size limit"));
    }

    let mut content = String::new();
    fs::File::open(path)?.read_to_string(&mut content)?;
    parse(&content).map(Some)
}

pub fn save(root: &Path, state: &SessionState) -> io::Result<()> {
    let state_dir = checked_state_dir(root, true)?;
    let target = state_dir.join(SESSION_FILE);
    let previous = state_dir.join(SESSION_PREVIOUS);
    let temp = state_dir.join(format!("session.v1.tmp-{}", std::process::id()));

    let payload = serialize(state)?;

    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "session target must be a regular non-symlink file",
            ));
        }
    }

    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(payload.as_bytes())?;
    file.sync_all()?;
    drop(file);

    let had_target = target.is_file();

    if had_target {
        match fs::remove_file(&previous) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(error);
            }
        }

        fs::rename(&target, &previous)?;
    }

    if let Err(error) = fs::rename(&temp, &target) {
        if had_target {
            let _ = fs::rename(&previous, &target);
        }
        let _ = fs::remove_file(&temp);
        return Err(error);
    }

    if had_target {
        let _ = fs::remove_file(previous);
    }

    Ok(())
}

fn checked_state_dir(root: &Path, create: bool) -> io::Result<PathBuf> {
    let canonical_root = fs::canonicalize(root)?;
    let state = canonical_root.join("state");

    match fs::symlink_metadata(&state) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "workspace state directory must not be a symlink",
                ));
            }
            if !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "workspace state path is not a directory",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
            fs::create_dir(&state)?;
        }
        Err(error) => return Err(error),
    }

    let canonical_state = fs::canonicalize(&state)?;
    if !canonical_state.starts_with(&canonical_root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "workspace state directory resolves outside workspace",
        ));
    }

    Ok(canonical_state)
}

fn serialize(state: &SessionState) -> io::Result<String> {
    validate_optional_path(state.browser_selected.as_deref())?;
    validate_optional_path(state.editor_path.as_deref())?;
    validate_relative(&state.browser_dir, true)?;

    Ok(format!(
        "{MAGIC}\nview={}\nbrowser_dir={}\nbrowser_selected={}\neditor_path={}\n",
        state.view.as_str(),
        encode_path(Some(&state.browser_dir))?,
        encode_path(state.browser_selected.as_deref())?,
        encode_path(state.editor_path.as_deref())?,
    ))
}

fn parse(content: &str) -> io::Result<SessionState> {
    let mut lines = content.lines();

    if lines.next() != Some(MAGIC) {
        return Err(invalid_data("invalid session header"));
    }

    let view = parse_field(lines.next(), "view=")?;
    let browser_dir = decode_path(parse_field(lines.next(), "browser_dir=")?)?
        .unwrap_or_default();
    let browser_selected =
        decode_path(parse_field(lines.next(), "browser_selected=")?)?;
    let editor_path = decode_path(parse_field(lines.next(), "editor_path=")?)?;

    if lines.next().is_some() {
        return Err(invalid_data("unexpected session fields"));
    }

    validate_relative(&browser_dir, true)?;
    validate_optional_path(browser_selected.as_deref())?;
    validate_optional_path(editor_path.as_deref())?;

    Ok(SessionState {
        view: SessionView::parse(view)?,
        browser_dir,
        browser_selected,
        editor_path,
    })
}

fn parse_field<'a>(line: Option<&'a str>, prefix: &str) -> io::Result<&'a str> {
    line.and_then(|line| line.strip_prefix(prefix))
        .ok_or_else(|| invalid_data("missing or malformed session field"))
}

fn encode_path(path: Option<&Path>) -> io::Result<String> {
    let Some(path) = path else {
        return Ok("-".to_string());
    };

    let text = path
        .to_str()
        .ok_or_else(|| invalid_data("session path is not valid UTF-8"))?;
    Ok(hex_encode(text.as_bytes()))
}

fn decode_path(value: &str) -> io::Result<Option<PathBuf>> {
    if value == "-" {
        return Ok(None);
    }

    let bytes = hex_decode(value)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid_data("session path is not valid UTF-8"))?;
    Ok(Some(PathBuf::from(text)))
}

fn validate_optional_path(path: Option<&Path>) -> io::Result<()> {
    if let Some(path) = path {
        validate_relative(path, false)?;
    }
    Ok(())
}

fn validate_relative(path: &Path, allow_empty: bool) -> io::Result<()> {
    if (!allow_empty && path.as_os_str().is_empty()) || path.is_absolute() {
        return Err(invalid_data("session path is not a valid relative path"));
    }

    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_data("session path contains a non-normal component"));
    }

    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> io::Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        return Err(invalid_data("session hex field has odd length"));
    }

    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn hex_nibble(value: u8) -> io::Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(invalid_data("session hex field contains invalid character")),
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-session-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("state")).expect("state");
        root
    }

    #[test]
    fn round_trip_restores_session_fields() {
        let root = temporary_workspace("roundtrip");
        let expected = SessionState {
            view: SessionView::Editor,
            browser_dir: PathBuf::from("data/documents"),
            browser_selected: Some(PathBuf::from("data/documents/note.txt")),
            editor_path: Some(PathBuf::from("data/documents/note.txt")),
        };

        save(&root, &expected).expect("save");
        let actual = load(&root).expect("load").expect("session");

        assert_eq!(actual, expected);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn missing_session_is_none() {
        let root = temporary_workspace("missing");
        assert!(load(&root).expect("load").is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn parser_rejects_parent_traversal() {
        let content = format!(
            "{MAGIC}\nview=files\nbrowser_dir={}\nbrowser_selected=-\neditor_path=-\n",
            hex_encode(b"../outside")
        );
        let error = parse(&content).expect_err("reject traversal");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parser_rejects_unknown_view() {
        let content =
            format!("{MAGIC}\nview=unknown\nbrowser_dir=\nbrowser_selected=-\neditor_path=-\n");
        let error = parse(&content).expect_err("reject view");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[test]
    fn save_refuses_symlink_state_directory() {
        use std::os::unix::fs::symlink;

        let root = temporary_workspace("state-link");
        fs::remove_dir(root.join("state")).expect("remove state");
        let outside = root.with_extension("outside");
        fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, root.join("state")).expect("symlink");

        let error = save(&root, &SessionState::default()).expect_err("reject state symlink");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

        fs::remove_file(root.join("state")).expect("remove link");
        fs::remove_dir_all(root).expect("cleanup root");
        fs::remove_dir_all(outside).expect("cleanup outside");
    }
}
