use crc32fast::Hasher as Crc32;
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use smallpeg::{self, Encoder, Limits};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const MAX_INPUT_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 64_000_000;
pub const MAX_DIMENSION: u32 = 32_768;
const MAX_FILTERED_BYTES: u64 = 256 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const EXPORT_DIR: &[&str] = &["data", "exports"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
}

impl ImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMode {
    SmartSafe,
    Balanced,
}

impl CompressionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::SmartSafe => "SMART SAFE",
            Self::Balanced => "BALANCED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataPolicy {
    Preserve,
    PrivacyClean,
}

impl MetadataPolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preserve => "PRESERVE",
            Self::PrivacyClean => "PRIVACY CLEAN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressionOptions {
    pub mode: CompressionMode,
    pub metadata_policy: MetadataPolicy,
    pub jpeg_quality: u8,
    pub png_effort: u32,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            mode: CompressionMode::SmartSafe,
            metadata_policy: MetadataPolicy::Preserve,
            jpeg_quality: 85,
            png_effort: 6,
        }
    }
}

#[derive(Debug)]
pub struct CompressionResult {
    pub source: PathBuf,
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub original_bytes: u64,
    pub candidate_bytes: u64,
    pub lossless: bool,
    pub metadata_cleaned: bool,
    output: Option<Vec<u8>>,
}

impl CompressionResult {
    pub fn has_output(&self) -> bool {
        self.output.is_some()
    }

    pub fn bytes_saved(&self) -> i64 {
        self.original_bytes as i64 - self.candidate_bytes as i64
    }

    pub fn output_bytes(&self) -> Option<&[u8]> {
        self.output.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCompressError {
    InvalidWorkspacePath,
    UnsafePath,
    TargetNotRegularFile,
    InputTooLarge,
    OutputTooLarge,
    UnsupportedFormat,
    InvalidImage,
    UnsupportedAnimation,
    UnsupportedInterlacedPng,
    ImageTooLarge,
    InvalidOptions,
    BalancedRequiresPrivacyClean,
    UnsupportedJpeg,
    NoOutput,
    Io,
}

impl fmt::Display for ImageCompressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidWorkspacePath => "image target must be a normal relative Workspace path",
            Self::UnsafePath => "image target resolves through an unsafe filesystem path",
            Self::TargetNotRegularFile => "image target is not a regular file",
            Self::InputTooLarge => "image exceeds the 256 MiB input limit",
            Self::OutputTooLarge => "compressed output exceeds the 512 MiB output limit",
            Self::UnsupportedFormat => "only JPEG and PNG are supported",
            Self::InvalidImage => "image is malformed or truncated",
            Self::UnsupportedAnimation => "animated PNG is not supported by this first image-compress boundary",
            Self::UnsupportedInterlacedPng => "interlaced PNG is not supported by this first image-compress boundary",
            Self::ImageTooLarge => "image dimensions or decoded size exceed the configured safety limit",
            Self::InvalidOptions => "image compression options are outside the supported range",
            Self::BalancedRequiresPrivacyClean => "BALANCED JPEG requires PRIVACY CLEAN because non-rendering metadata cannot be preserved across re-encoding",
            Self::UnsupportedJpeg => "JPEG coding is unsupported by the current native backend",
            Self::NoOutput => "there is no new compressed output to save",
            Self::Io => "image compression filesystem operation failed",
        };
        formatter.write_str(message)
    }
}

impl Error for ImageCompressError {}

pub fn compress_workspace_file(
    workspace_root: &Path,
    relative: &Path,
    options: CompressionOptions,
) -> Result<CompressionResult, ImageCompressError> {
    if !(1..=100).contains(&options.jpeg_quality) || !(1..=9).contains(&options.png_effort) {
        return Err(ImageCompressError::InvalidOptions);
    }

    let path = resolve_existing_file(workspace_root, relative)?;
    let before = fs::metadata(&path).map_err(|_| ImageCompressError::Io)?;
    if before.len() > MAX_INPUT_BYTES {
        return Err(ImageCompressError::InputTooLarge);
    }

    let mut file = File::open(&path).map_err(|_| ImageCompressError::Io)?;
    let mut data = Vec::with_capacity(before.len().min(MAX_INPUT_BYTES) as usize);
    (&mut file)
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|_| ImageCompressError::Io)?;
    if data.len() as u64 > MAX_INPUT_BYTES {
        return Err(ImageCompressError::InputTooLarge);
    }

    let after = fs::metadata(&path).map_err(|_| ImageCompressError::Io)?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(ImageCompressError::Io);
    }

    let mut result = if data.starts_with(&[0xff, 0xd8, 0xff]) {
        compress_jpeg(&data, options)?
    } else if data.starts_with(PNG_SIGNATURE) {
        compress_png(&data, options)?
    } else {
        return Err(ImageCompressError::UnsupportedFormat);
    };
    result.source = relative.to_path_buf();
    Ok(result)
}

pub fn save_result_to_exports(
    workspace_root: &Path,
    result: &CompressionResult,
) -> Result<PathBuf, ImageCompressError> {
    let output = result.output.as_deref().ok_or(ImageCompressError::NoOutput)?;
    validate_generated(
        result.format,
        output,
        result.width,
        result.height,
    )?;

    let exports = resolve_fixed_directory(workspace_root, EXPORT_DIR)?;
    let stem = result
        .source
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or(ImageCompressError::InvalidWorkspacePath)?;

    let pending = create_pending_file(&exports)?;
    let pending_path = pending.0;
    let mut pending_file = pending.1;
    let pending_result = (|| -> Result<(), ImageCompressError> {
        pending_file
            .write_all(output)
            .map_err(|_| ImageCompressError::Io)?;
        pending_file.sync_all().map_err(|_| ImageCompressError::Io)?;
        drop(pending_file);
        verify_file_bytes(&pending_path, output)
    })();
    if let Err(error) = pending_result {
        let _ = fs::remove_file(&pending_path);
        return Err(error);
    }

    let mut final_path = None;
    for suffix in 1..=10_000_u32 {
        let name = if suffix == 1 {
            format!("{stem}.optimized.{}", result.format.extension())
        } else {
            format!("{stem}.optimized-{suffix}.{}", result.format.extension())
        };
        let candidate = exports.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut final_file) => {
                if let Err(error) = final_file
                    .write_all(output)
                    .and_then(|_| final_file.sync_all())
                {
                    let _ = fs::remove_file(&candidate);
                    let _ = fs::remove_file(&pending_path);
                    let _ = error;
                    return Err(ImageCompressError::Io);
                }
                drop(final_file);
                if let Err(error) = verify_file_bytes(&candidate, output) {
                    let _ = fs::remove_file(&candidate);
                    let _ = fs::remove_file(&pending_path);
                    return Err(error);
                }
                final_path = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                let _ = fs::remove_file(&pending_path);
                return Err(ImageCompressError::Io);
            }
        }
    }

    let final_path = final_path.ok_or(ImageCompressError::Io)?;
    let _ = fs::remove_file(&pending_path);
    let relative = final_path
        .strip_prefix(&exports)
        .map_err(|_| ImageCompressError::Io)?;
    let mut exported = PathBuf::from("data");
    exported.push("exports");
    exported.push(relative);
    Ok(exported)
}

fn compress_jpeg(
    data: &[u8],
    options: CompressionOptions,
) -> Result<CompressionResult, ImageCompressError> {
    let info = smallpeg::jpeg_info(data).ok_or(ImageCompressError::InvalidImage)?;
    validate_dimensions(info.width, info.height)?;
    if !matches!(info.components, 1 | 3 | 4) {
        return Err(ImageCompressError::UnsupportedJpeg);
    }
    let limits = Limits::default().max_pixels(MAX_PIXELS);

    match options.mode {
        CompressionMode::SmartSafe => {
            let cleaned;
            let input = if options.metadata_policy == MetadataPolicy::PrivacyClean {
                cleaned = smallpeg::strip_metadata(data).ok_or(ImageCompressError::InvalidImage)?;
                cleaned.as_slice()
            } else {
                data
            };
            let candidate = smallpeg::optimize_jpeg_with(input, &limits)
                .ok_or(ImageCompressError::UnsupportedJpeg)?;
            if candidate.len() as u64 > MAX_OUTPUT_BYTES {
                return Err(ImageCompressError::OutputTooLarge);
            }

            let metadata_cleaned =
                options.metadata_policy == MetadataPolicy::PrivacyClean && input != data;
            let keep = if options.metadata_policy == MetadataPolicy::Preserve {
                candidate.len() < data.len()
            } else {
                candidate.as_slice() != data
            };
            let candidate_bytes = if keep {
                candidate.len() as u64
            } else {
                data.len() as u64
            };
            let output = keep.then_some(candidate);
            if let Some(bytes) = output.as_deref() {
                validate_generated(ImageFormat::Jpeg, bytes, info.width, info.height)?;
            }

            Ok(CompressionResult {
                source: PathBuf::new(),
                format: ImageFormat::Jpeg,
                width: info.width,
                height: info.height,
                original_bytes: data.len() as u64,
                candidate_bytes,
                lossless: true,
                metadata_cleaned,
                output,
            })
        }
        CompressionMode::Balanced => {
            if options.metadata_policy != MetadataPolicy::PrivacyClean {
                return Err(ImageCompressError::BalancedRequiresPrivacyClean);
            }

            let icc = extract_jpeg_icc(data)?;
            let mut encoder = Encoder::new(options.jpeg_quality).progressive(true);
            if let Some(profile) = icc.as_deref() {
                encoder = encoder.icc_profile(profile);
            }

            let (candidate, width, height) = match info.components {
                1 | 3 => {
                    let decoded = smallpeg::decode_jpeg_with(data, &limits)
                        .ok_or(ImageCompressError::UnsupportedJpeg)?;
                    let (pixels, width, height) = smallpeg::apply_orientation(
                        &decoded.pixels,
                        decoded.width,
                        decoded.height,
                        decoded.channels as usize,
                        info.orientation,
                    );
                    let encoded = if decoded.channels == 1 {
                        encoder.encode_gray(&pixels, width, height)
                    } else {
                        encoder.encode_rgb(&pixels, width, height)
                    };
                    (encoded, width, height)
                }
                4 => {
                    let decoded = smallpeg::decode_cmyk_with(data, &limits)
                        .ok_or(ImageCompressError::UnsupportedJpeg)?;
                    let (pixels, width, height) = smallpeg::apply_orientation(
                        &decoded.pixels,
                        decoded.width,
                        decoded.height,
                        4,
                        info.orientation,
                    );
                    (
                        encoder.encode_cmyk(&pixels, width, height, decoded.adobe),
                        width,
                        height,
                    )
                }
                _ => return Err(ImageCompressError::UnsupportedJpeg),
            };

            if candidate.len() as u64 > MAX_OUTPUT_BYTES {
                return Err(ImageCompressError::OutputTooLarge);
            }
            validate_generated(ImageFormat::Jpeg, &candidate, width, height)?;
            let output = (candidate.as_slice() != data).then_some(candidate);
            let candidate_bytes = output
                .as_ref()
                .map(|bytes| bytes.len() as u64)
                .unwrap_or(data.len() as u64);

            Ok(CompressionResult {
                source: PathBuf::new(),
                format: ImageFormat::Jpeg,
                width,
                height,
                original_bytes: data.len() as u64,
                candidate_bytes,
                lossless: false,
                metadata_cleaned: true,
                output,
            })
        }
    }
}

#[derive(Debug, Clone)]
struct PngChunk {
    start: usize,
    end: usize,
    data_start: usize,
    data_end: usize,
    kind: [u8; 4],
}

#[derive(Debug)]
struct ParsedPng {
    width: u32,
    height: u32,
    expected_filtered_bytes: usize,
    chunks: Vec<PngChunk>,
}

fn compress_png(
    data: &[u8],
    options: CompressionOptions,
) -> Result<CompressionResult, ImageCompressError> {
    let parsed = parse_png(data)?;
    let filtered = inflate_png(data, &parsed)?;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(options.png_effort));
    encoder
        .write_all(&filtered)
        .map_err(|_| ImageCompressError::InvalidImage)?;
    let compressed = encoder
        .finish()
        .map_err(|_| ImageCompressError::InvalidImage)?;

    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(PNG_SIGNATURE);
    let mut wrote_idat = false;
    let mut metadata_cleaned = false;

    for chunk in &parsed.chunks {
        if chunk.kind == *b"IDAT" {
            if !wrote_idat {
                append_png_chunk(&mut out, *b"IDAT", &compressed)?;
                wrote_idat = true;
            }
            continue;
        }

        if options.metadata_policy == MetadataPolicy::PrivacyClean
            && matches!(
                &chunk.kind,
                b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME"
            )
        {
            metadata_cleaned = true;
            continue;
        }

        out.extend_from_slice(&data[chunk.start..chunk.end]);
        if out.len() as u64 > MAX_OUTPUT_BYTES {
            return Err(ImageCompressError::OutputTooLarge);
        }
    }

    if !wrote_idat {
        return Err(ImageCompressError::InvalidImage);
    }
    if out.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(ImageCompressError::OutputTooLarge);
    }
    validate_generated(ImageFormat::Png, &out, parsed.width, parsed.height)?;

    let keep = if options.metadata_policy == MetadataPolicy::Preserve {
        out.len() < data.len()
    } else {
        metadata_cleaned || out.len() < data.len()
    };
    let candidate_bytes = if keep {
        out.len() as u64
    } else {
        data.len() as u64
    };

    Ok(CompressionResult {
        source: PathBuf::new(),
        format: ImageFormat::Png,
        width: parsed.width,
        height: parsed.height,
        original_bytes: data.len() as u64,
        candidate_bytes,
        lossless: true,
        metadata_cleaned,
        output: keep.then_some(out),
    })
}

fn parse_png(data: &[u8]) -> Result<ParsedPng, ImageCompressError> {
    if data.len() < PNG_SIGNATURE.len() || &data[..8] != PNG_SIGNATURE {
        return Err(ImageCompressError::InvalidImage);
    }

    let mut pos = 8usize;
    let mut chunks = Vec::new();
    let mut width = None;
    let mut height = None;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut interlace = 0u8;
    let mut seen_idat = false;
    let mut idat_ended = false;
    let mut seen_iend = false;

    while pos < data.len() {
        let header_end = pos.checked_add(8).ok_or(ImageCompressError::InvalidImage)?;
        if header_end > data.len() {
            return Err(ImageCompressError::InvalidImage);
        }
        let length = u32::from_be_bytes(
            data[pos..pos + 4]
                .try_into()
                .map_err(|_| ImageCompressError::InvalidImage)?,
        ) as usize;
        let data_start = pos + 8;
        let data_end = data_start
            .checked_add(length)
            .ok_or(ImageCompressError::InvalidImage)?;
        let end = data_end
            .checked_add(4)
            .ok_or(ImageCompressError::InvalidImage)?;
        if end > data.len() {
            return Err(ImageCompressError::InvalidImage);
        }

        let kind: [u8; 4] = data[pos + 4..pos + 8]
            .try_into()
            .map_err(|_| ImageCompressError::InvalidImage)?;
        let expected_crc = u32::from_be_bytes(
            data[data_end..end]
                .try_into()
                .map_err(|_| ImageCompressError::InvalidImage)?,
        );
        let mut crc = Crc32::new();
        crc.update(&kind);
        crc.update(&data[data_start..data_end]);
        if crc.finalize() != expected_crc {
            return Err(ImageCompressError::InvalidImage);
        }

        if chunks.is_empty() {
            if kind != *b"IHDR" || length != 13 {
                return Err(ImageCompressError::InvalidImage);
            }
            let w = u32::from_be_bytes(
                data[data_start..data_start + 4]
                    .try_into()
                    .map_err(|_| ImageCompressError::InvalidImage)?,
            );
            let h = u32::from_be_bytes(
                data[data_start + 4..data_start + 8]
                    .try_into()
                    .map_err(|_| ImageCompressError::InvalidImage)?,
            );
            width = Some(w);
            height = Some(h);
            bit_depth = data[data_start + 8];
            color_type = data[data_start + 9];
            if data[data_start + 10] != 0 || data[data_start + 11] != 0 {
                return Err(ImageCompressError::InvalidImage);
            }
            interlace = data[data_start + 12];
        } else if kind == *b"IHDR" {
            return Err(ImageCompressError::InvalidImage);
        }

        if kind == *b"acTL" {
            return Err(ImageCompressError::UnsupportedAnimation);
        }
        if kind == *b"IDAT" {
            if idat_ended {
                return Err(ImageCompressError::InvalidImage);
            }
            seen_idat = true;
        } else if seen_idat && kind != *b"IEND" {
            idat_ended = true;
        }

        chunks.push(PngChunk {
            start: pos,
            end,
            data_start,
            data_end,
            kind,
        });

        pos = end;
        if kind == *b"IEND" {
            if length != 0 || pos != data.len() {
                return Err(ImageCompressError::InvalidImage);
            }
            seen_iend = true;
            break;
        }
    }

    if !seen_idat || !seen_iend {
        return Err(ImageCompressError::InvalidImage);
    }
    if interlace != 0 {
        return Err(ImageCompressError::UnsupportedInterlacedPng);
    }

    let width = width.ok_or(ImageCompressError::InvalidImage)?;
    let height = height.ok_or(ImageCompressError::InvalidImage)?;
    validate_dimensions(width, height)?;

    let channels = match color_type {
        0 if matches!(bit_depth, 1 | 2 | 4 | 8 | 16) => 1u64,
        2 if matches!(bit_depth, 8 | 16) => 3,
        3 if matches!(bit_depth, 1 | 2 | 4 | 8) => 1,
        4 if matches!(bit_depth, 8 | 16) => 2,
        6 if matches!(bit_depth, 8 | 16) => 4,
        _ => return Err(ImageCompressError::InvalidImage),
    };

    let row_bits = (width as u64)
        .checked_mul(channels)
        .and_then(|value| value.checked_mul(bit_depth as u64))
        .ok_or(ImageCompressError::ImageTooLarge)?;
    let row_bytes = row_bits
        .checked_add(7)
        .ok_or(ImageCompressError::ImageTooLarge)?
        / 8;
    let filtered = (height as u64)
        .checked_mul(
            row_bytes
                .checked_add(1)
                .ok_or(ImageCompressError::ImageTooLarge)?,
        )
        .ok_or(ImageCompressError::ImageTooLarge)?;
    if filtered > MAX_FILTERED_BYTES || filtered > usize::MAX as u64 {
        return Err(ImageCompressError::ImageTooLarge);
    }

    Ok(ParsedPng {
        width,
        height,
        expected_filtered_bytes: filtered as usize,
        chunks,
    })
}

fn inflate_png(data: &[u8], parsed: &ParsedPng) -> Result<Vec<u8>, ImageCompressError> {
    let mut idat = Vec::new();
    for chunk in &parsed.chunks {
        if chunk.kind == *b"IDAT" {
            let part = &data[chunk.data_start..chunk.data_end];
            let next = idat
                .len()
                .checked_add(part.len())
                .ok_or(ImageCompressError::InputTooLarge)?;
            if next as u64 > MAX_INPUT_BYTES {
                return Err(ImageCompressError::InputTooLarge);
            }
            idat.extend_from_slice(part);
        }
    }

    let mut decoder = ZlibDecoder::new(idat.as_slice());
    let mut filtered = Vec::with_capacity(parsed.expected_filtered_bytes);
    {
        let mut limited = (&mut decoder).take(parsed.expected_filtered_bytes as u64 + 1);
        limited
            .read_to_end(&mut filtered)
            .map_err(|_| ImageCompressError::InvalidImage)?;
    }
    if filtered.len() != parsed.expected_filtered_bytes {
        return Err(ImageCompressError::InvalidImage);
    }
    if decoder.total_in() as usize != idat.len() {
        return Err(ImageCompressError::InvalidImage);
    }
    Ok(filtered)
}

fn append_png_chunk(
    output: &mut Vec<u8>,
    kind: [u8; 4],
    data: &[u8],
) -> Result<(), ImageCompressError> {
    if data.len() > u32::MAX as usize {
        return Err(ImageCompressError::OutputTooLarge);
    }
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(&kind);
    output.extend_from_slice(data);
    let mut crc = Crc32::new();
    crc.update(&kind);
    crc.update(data);
    output.extend_from_slice(&crc.finalize().to_be_bytes());
    if output.len() as u64 > MAX_OUTPUT_BYTES {
        return Err(ImageCompressError::OutputTooLarge);
    }
    Ok(())
}

fn validate_generated(
    format: ImageFormat,
    bytes: &[u8],
    expected_width: u32,
    expected_height: u32,
) -> Result<(), ImageCompressError> {
    match format {
        ImageFormat::Jpeg => {
            let info = smallpeg::jpeg_info(bytes).ok_or(ImageCompressError::InvalidImage)?;
            if info.width != expected_width || info.height != expected_height {
                return Err(ImageCompressError::InvalidImage);
            }
            let limits = Limits::default().max_pixels(MAX_PIXELS);
            match info.components {
                1 | 3 => {
                    if smallpeg::decode_jpeg_with(bytes, &limits).is_none() {
                        return Err(ImageCompressError::InvalidImage);
                    }
                }
                4 => {
                    if smallpeg::decode_cmyk_with(bytes, &limits).is_none() {
                        return Err(ImageCompressError::InvalidImage);
                    }
                }
                _ => return Err(ImageCompressError::UnsupportedJpeg),
            }
        }
        ImageFormat::Png => {
            let parsed = parse_png(bytes)?;
            if parsed.width != expected_width || parsed.height != expected_height {
                return Err(ImageCompressError::InvalidImage);
            }
            let _ = inflate_png(bytes, &parsed)?;
        }
    }
    Ok(())
}

fn extract_jpeg_icc(data: &[u8]) -> Result<Option<Vec<u8>>, ImageCompressError> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return Err(ImageCompressError::InvalidImage);
    }

    let mut pos = 2usize;
    let mut total: Option<u8> = None;
    let mut parts = BTreeMap::<u8, Vec<u8>>::new();

    while pos < data.len() {
        if data[pos] != 0xff {
            return Err(ImageCompressError::InvalidImage);
        }
        while pos < data.len() && data[pos] == 0xff {
            pos += 1;
        }
        let marker = *data.get(pos).ok_or(ImageCompressError::InvalidImage)?;
        pos += 1;

        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }

        let length_end = pos.checked_add(2).ok_or(ImageCompressError::InvalidImage)?;
        if length_end > data.len() {
            return Err(ImageCompressError::InvalidImage);
        }
        let length = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if length < 2 {
            return Err(ImageCompressError::InvalidImage);
        }
        let segment_end = pos
            .checked_add(length)
            .ok_or(ImageCompressError::InvalidImage)?;
        if segment_end > data.len() {
            return Err(ImageCompressError::InvalidImage);
        }
        let payload = &data[pos + 2..segment_end];

        if marker == 0xe2 && payload.starts_with(b"ICC_PROFILE\0") {
            if payload.len() < 14 {
                return Err(ImageCompressError::InvalidImage);
            }
            let seq = payload[12];
            let count = payload[13];
            if seq == 0 || count == 0 || seq > count {
                return Err(ImageCompressError::InvalidImage);
            }
            if let Some(previous) = total {
                if previous != count {
                    return Err(ImageCompressError::InvalidImage);
                }
            } else {
                total = Some(count);
            }
            if parts.insert(seq, payload[14..].to_vec()).is_some() {
                return Err(ImageCompressError::InvalidImage);
            }
        }

        pos = segment_end;
    }

    let Some(total) = total else {
        return Ok(None);
    };
    if parts.len() != total as usize {
        return Err(ImageCompressError::InvalidImage);
    }
    let mut profile = Vec::new();
    for seq in 1..=total {
        let part = parts.get(&seq).ok_or(ImageCompressError::InvalidImage)?;
        if profile
            .len()
            .checked_add(part.len())
            .ok_or(ImageCompressError::ImageTooLarge)? as u64
            > MAX_INPUT_BYTES
        {
            return Err(ImageCompressError::ImageTooLarge);
        }
        profile.extend_from_slice(part);
    }
    Ok(Some(profile))
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), ImageCompressError> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || (width as u64)
            .checked_mul(height as u64)
            .ok_or(ImageCompressError::ImageTooLarge)?
            > MAX_PIXELS
    {
        return Err(ImageCompressError::ImageTooLarge);
    }
    Ok(())
}

fn resolve_existing_file(
    workspace_root: &Path,
    relative: &Path,
) -> Result<PathBuf, ImageCompressError> {
    validate_relative_path(relative)?;
    let canonical_root =
        fs::canonicalize(workspace_root).map_err(|_| ImageCompressError::Io)?;
    if !fs::metadata(&canonical_root)
        .map_err(|_| ImageCompressError::Io)?
        .is_dir()
    {
        return Err(ImageCompressError::InvalidWorkspacePath);
    }

    let mut candidate = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(ImageCompressError::InvalidWorkspacePath);
        };
        candidate.push(segment);
        let metadata =
            fs::symlink_metadata(&candidate).map_err(|_| ImageCompressError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(ImageCompressError::UnsafePath);
        }
    }

    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|_| ImageCompressError::Io)?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(ImageCompressError::UnsafePath);
    }
    if !fs::metadata(&canonical_candidate)
        .map_err(|_| ImageCompressError::Io)?
        .is_file()
    {
        return Err(ImageCompressError::TargetNotRegularFile);
    }
    Ok(canonical_candidate)
}

fn resolve_fixed_directory(
    workspace_root: &Path,
    relative: &[&str],
) -> Result<PathBuf, ImageCompressError> {
    let canonical_root =
        fs::canonicalize(workspace_root).map_err(|_| ImageCompressError::Io)?;
    let mut candidate = canonical_root.clone();
    for segment in relative {
        candidate.push(segment);
        let metadata =
            fs::symlink_metadata(&candidate).map_err(|_| ImageCompressError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ImageCompressError::UnsafePath);
        }
    }
    let canonical =
        fs::canonicalize(&candidate).map_err(|_| ImageCompressError::Io)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(ImageCompressError::UnsafePath);
    }
    Ok(canonical)
}

fn validate_relative_path(relative: &Path) -> Result<(), ImageCompressError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(ImageCompressError::InvalidWorkspacePath);
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ImageCompressError::InvalidWorkspacePath);
    }
    Ok(())
}

fn create_pending_file(
    exports: &Path,
) -> Result<(PathBuf, File), ImageCompressError> {
    for counter in 0..1000_u32 {
        let candidate = exports.join(format!(
            ".image-compress-{}-{counter:03}.pending",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(ImageCompressError::Io),
        }
    }
    Err(ImageCompressError::Io)
}

fn verify_file_bytes(path: &Path, expected: &[u8]) -> Result<(), ImageCompressError> {
    let mut file = File::open(path).map_err(|_| ImageCompressError::Io)?;
    let metadata = file.metadata().map_err(|_| ImageCompressError::Io)?;
    if metadata.len() != expected.len() as u64 {
        return Err(ImageCompressError::Io);
    }
    let mut offset = 0usize;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| ImageCompressError::Io)?;
        if read == 0 {
            break;
        }
        let end = offset.checked_add(read).ok_or(ImageCompressError::Io)?;
        if expected.get(offset..end) != Some(&buffer[..read]) {
            return Err(ImageCompressError::Io);
        }
        offset = end;
    }
    if offset != expected.len() {
        return Err(ImageCompressError::Io);
    }
    Ok(())
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
            "dangomushi-image-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("data/exports")).expect("exports");
        root
    }

    fn inefficient_png(width: u32, height: u32) -> Vec<u8> {
        let mut filtered = Vec::new();
        for _ in 0..height {
            filtered.push(0);
            for _ in 0..width {
                filtered.extend_from_slice(&[48, 96, 144, 255]);
            }
        }
        let mut z = ZlibEncoder::new(Vec::new(), Compression::none());
        z.write_all(&filtered).expect("zlib");
        let idat = z.finish().expect("zlib finish");

        let mut png = PNG_SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        append_png_chunk(&mut png, *b"IHDR", &ihdr).expect("ihdr");
        append_png_chunk(&mut png, *b"IDAT", &idat).expect("idat");
        append_png_chunk(&mut png, *b"IEND", &[]).expect("iend");
        png
    }

    #[test]
    fn png_smart_safe_recompresses_without_changing_filtered_pixels() {
        let root = temporary_workspace("png");
        let source = inefficient_png(128, 64);
        fs::write(root.join("photo.png"), &source).expect("seed");

        let result = compress_workspace_file(
            &root,
            Path::new("photo.png"),
            CompressionOptions {
                png_effort: 9,
                ..CompressionOptions::default()
            },
        )
        .expect("compress");
        assert_eq!(result.format, ImageFormat::Png);
        assert!(result.lossless);
        assert!(result.has_output());
        assert!(result.candidate_bytes < result.original_bytes);

        let source_parsed = parse_png(&source).expect("source parse");
        let output = result.output_bytes().expect("output");
        let output_parsed = parse_png(output).expect("output parse");
        assert_eq!(
            inflate_png(&source, &source_parsed).expect("source pixels"),
            inflate_png(output, &output_parsed).expect("output pixels")
        );

        let exported = save_result_to_exports(&root, &result).expect("save");
        assert_eq!(exported, PathBuf::from("data/exports/photo.optimized.png"));
        assert_eq!(fs::read(root.join("photo.png")).expect("source"), source);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn png_export_never_overwrites_an_existing_result() {
        let root = temporary_workspace("collision");
        fs::write(root.join("photo.png"), inefficient_png(128, 64)).expect("seed");
        let result = compress_workspace_file(
            &root,
            Path::new("photo.png"),
            CompressionOptions {
                png_effort: 9,
                ..CompressionOptions::default()
            },
        )
        .expect("compress");
        let first = save_result_to_exports(&root, &result).expect("first");
        let second = save_result_to_exports(&root, &result).expect("second");
        assert_ne!(first, second);
        assert_eq!(second, PathBuf::from("data/exports/photo.optimized-2.png"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn jpeg_privacy_clean_is_lossless() {
        let root = temporary_workspace("jpeg");
        let pixels = vec![120u8; 64 * 64 * 3];
        let jpeg = Encoder::new(88).progressive(false).encode_rgb(&pixels, 64, 64);
        let mut tagged = Vec::new();
        tagged.extend_from_slice(&jpeg[..2]);
        tagged.extend_from_slice(&[0xff, 0xfe, 0x00, 0x18]);
        tagged.extend_from_slice(b"private camera comment");
        tagged.extend_from_slice(&jpeg[2..]);
        fs::write(root.join("photo.jpg"), &tagged).expect("seed");

        let result = compress_workspace_file(
            &root,
            Path::new("photo.jpg"),
            CompressionOptions {
                metadata_policy: MetadataPolicy::PrivacyClean,
                ..CompressionOptions::default()
            },
        )
        .expect("compress");
        let output = result.output_bytes().expect("clean output");
        assert!(result.lossless);
        assert!(result.metadata_cleaned);
        assert_eq!(
            smallpeg::decode_jpeg(&tagged).expect("source decode").pixels,
            smallpeg::decode_jpeg(output).expect("output decode").pixels
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn balanced_jpeg_requires_explicit_privacy_clean() {
        let root = temporary_workspace("balanced");
        let jpeg = Encoder::new(90)
            .progressive(false)
            .encode_rgb(&vec![90u8; 32 * 32 * 3], 32, 32);
        fs::write(root.join("photo.jpg"), jpeg).expect("seed");

        let error = compress_workspace_file(
            &root,
            Path::new("photo.jpg"),
            CompressionOptions {
                mode: CompressionMode::Balanced,
                ..CompressionOptions::default()
            },
        )
        .expect_err("preserve must be rejected");
        assert_eq!(error, ImageCompressError::BalancedRequiresPrivacyClean);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_parent_traversal() {
        let root = temporary_workspace("traversal");
        let error = compress_workspace_file(
            &root,
            Path::new("../outside.png"),
            CompressionOptions::default(),
        )
        .expect_err("reject traversal");
        assert_eq!(error, ImageCompressError::InvalidWorkspacePath);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
