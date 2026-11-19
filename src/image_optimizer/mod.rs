use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::metadata::Orientation;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageEncoder, ImageFormat as RustImageFormat, ImageReader};
use jpeg_encoder::{ColorType as JpegColorType, Encoder as JpegEncoder};
use oxipng::{Options as OxiOptions, StripChunks};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::panic::{self, AssertUnwindSafe};

pub mod worker;

pub const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 50_000_000;
const MAX_DECOMPRESSED_BYTES: usize = 256 * 1024 * 1024;
const MAX_EXPORT_ATTEMPTS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
}

impl ImageFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::WebP => "WEBP",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Keep,
    Jpeg,
    Png,
    WebP,
}

impl OutputFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Keep => "KEEP",
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::WebP => "WEBP",
        }
    }

    pub fn resolve(self, source: ImageFormat) -> ImageFormat {
        match self {
            Self::Keep => source,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Png => ImageFormat::Png,
            Self::WebP => ImageFormat::WebP,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Keep => Self::Jpeg,
            Self::Jpeg => Self::Png,
            Self::Png => Self::WebP,
            Self::WebP => Self::Keep,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Keep => Self::WebP,
            Self::Jpeg => Self::Keep,
            Self::Png => Self::Jpeg,
            Self::WebP => Self::Png,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Fast,
    Balanced,
    High,
}

impl Effort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "FAST",
            Self::Balanced => "BALANCED",
            Self::High => "HIGH",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Fast => Self::Balanced,
            Self::Balanced => Self::High,
            Self::High => Self::Fast,
        }
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Fast => Self::High,
            Self::Balanced => Self::Fast,
            Self::High => Self::Balanced,
        }
    }

    fn oxipng_level(self) -> u8 {
        match self {
            Self::Fast => 2,
            Self::Balanced => 4,
            Self::High => 6,
        }
    }

    fn webp_method(self) -> u8 {
        match self {
            Self::Fast => 2,
            Self::Balanced => 4,
            Self::High => 6,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptimizeSettings {
    pub format: OutputFormat,
    pub quality: u8,
    pub effort: Effort,
}

impl Default for OptimizeSettings {
    fn default() -> Self {
        Self {
            format: OutputFormat::Keep,
            quality: 82,
            effort: Effort::Balanced,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    pub source_bytes: u64,
    pub has_alpha: bool,
    pub animated: bool,
    pub icc_present: bool,
    pub exif_present: bool,
    pub xmp_present: bool,
}

impl ImageInfo {
    pub fn megapixels(&self) -> f64 {
        (u64::from(self.width) * u64::from(self.height)) as f64 / 1_000_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFingerprint {
    pub bytes: u64,
    pub sha256: [u8; 32],
}

impl SourceFingerprint {
    pub fn sha256_hex(&self) -> String {
        hex_digest(&self.sha256)
    }
}

pub struct CandidateArtifact {
    pub source_relative: PathBuf,
    pub source_info: ImageInfo,
    pub source_fingerprint: SourceFingerprint,
    pub output_format: ImageFormat,
    pub output_bytes: Vec<u8>,
    pub output_sha256: [u8; 32],
    pub before_preview: DynamicImage,
    pub after_preview: DynamicImage,
    pub color_profile_preserved: bool,
    pub private_metadata_stripped: bool,
}

impl CandidateArtifact {
    pub fn output_sha256_hex(&self) -> String {
        hex_digest(&self.output_sha256)
    }

    pub fn output_bytes_len(&self) -> usize {
        self.output_bytes.len()
    }

    pub fn reduction_percent(&self) -> f64 {
        if self.source_info.source_bytes == 0 {
            return 0.0;
        }
        (1.0 - self.output_bytes.len() as f64 / self.source_info.source_bytes as f64) * 100.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    pub relative_path: PathBuf,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

impl ExportSummary {
    pub fn sha256_hex(&self) -> String {
        hex_digest(&self.sha256)
    }
}

#[derive(Debug)]
pub enum OptimizeError {
    Io(io::Error),
    InvalidPath,
    UnsupportedFormat,
    AnimatedUnsupported,
    SourceTooLarge { bytes: u64, max: u64 },
    TooManyPixels { pixels: u64, max: u64 },
    InvalidQuality(u8),
    AlphaToJpeg,
    DimensionsUnsupported,
    Decode(String),
    Encode(String),
    CodecPanic(&'static str),
    OutputTooLarge { bytes: usize, max: usize },
    SourceChanged,
    ExportNameExhausted,
}

impl fmt::Display for OptimizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::InvalidPath => write!(f, "workspace image path is invalid or escapes the workspace"),
            Self::UnsupportedFormat => write!(f, "only JPEG, PNG and still WebP are supported"),
            Self::AnimatedUnsupported => write!(f, "animated images are not supported; no file was modified"),
            Self::SourceTooLarge { bytes, max } => {
                write!(f, "image source is too large: {bytes} bytes (max {max})")
            }
            Self::TooManyPixels { pixels, max } => {
                write!(f, "image has too many pixels: {pixels} (max {max})")
            }
            Self::InvalidQuality(value) => write!(f, "quality must be in 1..=100, got {value}"),
            Self::AlphaToJpeg => write!(f, "JPEG does not support alpha; choose PNG or WebP"),
            Self::DimensionsUnsupported => write!(f, "image dimensions are unsupported by the selected codec"),
            Self::Decode(error) => write!(f, "image decode failed: {error}"),
            Self::Encode(error) => write!(f, "image encode failed: {error}"),
            Self::CodecPanic(codec) => write!(f, "{codec} codec aborted while handling this image"),
            Self::OutputTooLarge { bytes, max } => {
                write!(f, "encoded output is too large: {bytes} bytes (max {max})")
            }
            Self::SourceChanged => write!(f, "source image changed after preview; generate a new preview"),
            Self::ExportNameExhausted => write!(f, "could not allocate a unique export filename"),
        }
    }
}

impl std::error::Error for OptimizeError {}

impl From<io::Error> for OptimizeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

struct DecodedSource {
    info: ImageInfo,
    fingerprint: SourceFingerprint,
    image: DynamicImage,
    icc_profile: Option<Vec<u8>>,
}

pub fn probe_workspace_image(root: &Path, relative: &Path) -> Result<ImageInfo, OptimizeError> {
    let bytes = read_workspace_source(root, relative)?;
    probe_bytes(&bytes)
}

pub fn optimize_workspace_image(
    root: &Path,
    relative: &Path,
    settings: OptimizeSettings,
) -> Result<CandidateArtifact, OptimizeError> {
    if !(1..=100).contains(&settings.quality) {
        return Err(OptimizeError::InvalidQuality(settings.quality));
    }

    let source_bytes = read_workspace_source(root, relative)?;
    let source_sha256 = sha256(&source_bytes);
    let source_fingerprint = SourceFingerprint {
        bytes: source_bytes.len() as u64,
        sha256: source_sha256,
    };
    let mut decoded = decode_source(&source_bytes, source_fingerprint.clone())?;

    let output_format = settings.format.resolve(decoded.info.format);
    if output_format == ImageFormat::Jpeg && decoded.info.has_alpha {
        return Err(OptimizeError::AlphaToJpeg);
    }

    let output_bytes = match output_format {
        ImageFormat::Png => encode_png(&source_bytes, &decoded, settings)?,
        ImageFormat::Jpeg => encode_jpeg(&decoded, settings)?,
        ImageFormat::WebP => encode_webp(&decoded, settings)?,
    };
    ensure_output_size(output_bytes.len())?;

    let after_preview = decode_preview(&output_bytes, output_format)?;
    let before_preview = std::mem::replace(&mut decoded.image, DynamicImage::new_rgba8(1, 1));

    let color_profile_preserved =
        decoded.icc_profile.is_none() || !matches!(output_format, ImageFormat::WebP);
    let private_metadata_stripped = decoded.info.exif_present || decoded.info.xmp_present;

    Ok(CandidateArtifact {
        source_relative: relative.to_path_buf(),
        source_info: decoded.info,
        source_fingerprint,
        output_format,
        output_sha256: sha256(&output_bytes),
        output_bytes,
        before_preview,
        after_preview,
        color_profile_preserved,
        private_metadata_stripped,
    })
}

pub fn export_candidate(root: &Path, candidate: &CandidateArtifact) -> Result<ExportSummary, OptimizeError> {
    let current = read_workspace_source(root, &candidate.source_relative)?;
    if current.len() as u64 != candidate.source_fingerprint.bytes
        || sha256(&current) != candidate.source_fingerprint.sha256
    {
        return Err(OptimizeError::SourceChanged);
    }

    let canonical_root = fs::canonicalize(root)?;
    let exports = root.join("data").join("exports");
    fs::create_dir_all(&exports)?;
    let canonical_exports = fs::canonicalize(&exports)?;
    if !canonical_exports.starts_with(&canonical_root) {
        return Err(OptimizeError::InvalidPath);
    }

    let stem = candidate
        .source_relative
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("image");
    let extension = candidate.output_format.extension();

    for index in 1..=MAX_EXPORT_ATTEMPTS {
        let name = if index == 1 {
            format!("{stem}-optimized.{extension}")
        } else {
            format!("{stem}-optimized-{index}.{extension}")
        };
        let output = canonical_exports.join(&name);
        match OpenOptions::new().write(true).create_new(true).open(&output) {
            Ok(mut file) => {
                file.write_all(&candidate.output_bytes)?;
                file.sync_all()?;
                return Ok(ExportSummary {
                    relative_path: PathBuf::from("data").join("exports").join(name),
                    bytes: candidate.output_bytes.len() as u64,
                    sha256: candidate.output_sha256,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(OptimizeError::Io(error)),
        }
    }

    Err(OptimizeError::ExportNameExhausted)
}

fn read_workspace_source(root: &Path, relative: &Path) -> Result<Vec<u8>, OptimizeError> {
    validate_relative(relative)?;
    let canonical_root = fs::canonicalize(root)?;
    let joined = root.join(relative);
    let metadata = fs::symlink_metadata(&joined)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(OptimizeError::InvalidPath);
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(OptimizeError::SourceTooLarge {
            bytes: metadata.len(),
            max: MAX_SOURCE_BYTES,
        });
    }

    let canonical = fs::canonicalize(&joined)?;
    if !canonical.starts_with(&canonical_root) {
        return Err(OptimizeError::InvalidPath);
    }

    let mut file = File::open(canonical)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(OptimizeError::SourceTooLarge {
            bytes: bytes.len() as u64,
            max: MAX_SOURCE_BYTES,
        });
    }
    Ok(bytes)
}

fn validate_relative(relative: &Path) -> Result<(), OptimizeError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(OptimizeError::InvalidPath);
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(OptimizeError::InvalidPath);
    }
    Ok(())
}

fn probe_bytes(bytes: &[u8]) -> Result<ImageInfo, OptimizeError> {
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(OptimizeError::SourceTooLarge {
            bytes: bytes.len() as u64,
            max: MAX_SOURCE_BYTES,
        });
    }

    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| OptimizeError::Decode(error.to_string()))?;
    let format = map_image_format(reader.format())?;
    let animated = detect_animation(format, bytes);
    if animated {
        return Err(OptimizeError::AnimatedUnsupported);
    }

    let mut decoder = reader
        .into_decoder()
        .map_err(|error| OptimizeError::Decode(error.to_string()))?;
    let (width, height) = decoder.dimensions();
    validate_dimensions(width, height)?;
    let has_alpha = decoder.color_type().has_alpha();
    let icc_present = decoder.icc_profile().ok().flatten().is_some();
    let exif_present = decoder.exif_metadata().ok().flatten().is_some();
    let xmp_present = decoder.xmp_metadata().ok().flatten().is_some();

    Ok(ImageInfo {
        format,
        width,
        height,
        source_bytes: bytes.len() as u64,
        has_alpha,
        animated: false,
        icc_present,
        exif_present,
        xmp_present,
    })
}

fn decode_source(bytes: &[u8], fingerprint: SourceFingerprint) -> Result<DecodedSource, OptimizeError> {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<DecodedSource, OptimizeError> {
        let reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| OptimizeError::Decode(error.to_string()))?;
        let format = map_image_format(reader.format())?;
        if detect_animation(format, bytes) {
            return Err(OptimizeError::AnimatedUnsupported);
        }

        let mut decoder = reader
            .into_decoder()
            .map_err(|error| OptimizeError::Decode(error.to_string()))?;
        let (raw_width, raw_height) = decoder.dimensions();
        validate_dimensions(raw_width, raw_height)?;
        let has_alpha = decoder.color_type().has_alpha();
        let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
        let icc_profile = decoder.icc_profile().ok().flatten();
        let exif_present = decoder.exif_metadata().ok().flatten().is_some();
        let xmp_present = decoder.xmp_metadata().ok().flatten().is_some();

        let mut image = DynamicImage::from_decoder(decoder)
            .map_err(|error| OptimizeError::Decode(error.to_string()))?;
        image.apply_orientation(orientation);
        let (width, height) = image.dimensions();
        validate_dimensions(width, height)?;

        Ok(DecodedSource {
            info: ImageInfo {
                format,
                width,
                height,
                source_bytes: fingerprint.bytes,
                has_alpha,
                animated: false,
                icc_present: icc_profile.is_some(),
                exif_present,
                xmp_present,
            },
            fingerprint,
            image,
            icc_profile,
        })
    }));

    result.unwrap_or(Err(OptimizeError::CodecPanic("image decoder")))
}

fn decode_preview(bytes: &[u8], expected: ImageFormat) -> Result<DynamicImage, OptimizeError> {
    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<DynamicImage, OptimizeError> {
        let reader = ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|error| OptimizeError::Decode(error.to_string()))?;
        if map_image_format(reader.format())? != expected {
            return Err(OptimizeError::Decode("encoded output format mismatch".to_string()));
        }
        let image = reader
            .decode()
            .map_err(|error| OptimizeError::Decode(error.to_string()))?;
        validate_dimensions(image.width(), image.height())?;
        Ok(image)
    }));
    result.unwrap_or(Err(OptimizeError::CodecPanic("image decoder")))
}

fn encode_png(
    source_bytes: &[u8],
    decoded: &DecodedSource,
    settings: OptimizeSettings,
) -> Result<Vec<u8>, OptimizeError> {
    let direct_lossless = decoded.info.format == ImageFormat::Png
        && !decoded.info.exif_present
        && !decoded.info.xmp_present;

    let initial = if direct_lossless {
        source_bytes.to_vec()
    } else {
        encode_png_pixels(decoded)?
    };

    let mut options = OxiOptions::from_preset(settings.effort.oxipng_level());
    options.force = true;
    options.optimize_alpha = false;
    options.strip = StripChunks::Safe;
    options.max_decompressed_size = Some(MAX_DECOMPRESSED_BYTES);

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        oxipng::optimize_from_memory(&initial, &options)
    }));
    let output = match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => return Err(OptimizeError::Encode(error.to_string())),
        Err(_) => return Err(OptimizeError::CodecPanic("oxipng")),
    };
    ensure_output_size(output.len())?;
    Ok(output)
}

fn encode_png_pixels(decoded: &DecodedSource) -> Result<Vec<u8>, OptimizeError> {
    let mut output = Vec::new();
    let compression = CompressionType::Best;
    let filter = FilterType::Adaptive;
    let mut encoder = PngEncoder::new_with_quality(&mut output, compression, filter);
    if let Some(profile) = decoded.icc_profile.clone() {
        encoder
            .set_icc_profile(profile)
            .map_err(|error| OptimizeError::Encode(error.to_string()))?;
    }

    if decoded.info.has_alpha {
        let rgba = decoded.image.to_rgba8();
        encoder
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|error| OptimizeError::Encode(error.to_string()))?;
    } else {
        let rgb = decoded.image.to_rgb8();
        encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|error| OptimizeError::Encode(error.to_string()))?;
    }
    Ok(output)
}

fn encode_jpeg(decoded: &DecodedSource, settings: OptimizeSettings) -> Result<Vec<u8>, OptimizeError> {
    if decoded.info.has_alpha {
        return Err(OptimizeError::AlphaToJpeg);
    }
    let width = u16::try_from(decoded.info.width).map_err(|_| OptimizeError::DimensionsUnsupported)?;
    let height = u16::try_from(decoded.info.height).map_err(|_| OptimizeError::DimensionsUnsupported)?;
    let rgb = decoded.image.to_rgb8();
    let mut output = Vec::new();

    let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<(), OptimizeError> {
        let mut encoder = JpegEncoder::new(&mut output, settings.quality);
        encoder.set_optimized_huffman_tables(!matches!(settings.effort, Effort::Fast));
        if matches!(settings.effort, Effort::High) {
            encoder.set_progressive(true);
        }
        if let Some(profile) = decoded.icc_profile.as_deref() {
            encoder
                .add_icc_profile(profile)
                .map_err(|error| OptimizeError::Encode(error.to_string()))?;
        }
        encoder
            .encode(rgb.as_raw(), width, height, JpegColorType::Rgb)
            .map_err(|error| OptimizeError::Encode(error.to_string()))
    }));
    match result {
        Ok(result) => result?,
        Err(_) => return Err(OptimizeError::CodecPanic("jpeg-encoder")),
    }

    ensure_output_size(output.len())?;
    Ok(output)
}

fn encode_webp(decoded: &DecodedSource, settings: OptimizeSettings) -> Result<Vec<u8>, OptimizeError> {
    if decoded.info.width > 16_383 || decoded.info.height > 16_383 {
        return Err(OptimizeError::DimensionsUnsupported);
    }

    let rgba = decoded.image.to_rgba8();
    let image = webp_rust::ImageBuffer {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        rgba: rgba.into_raw(),
    };
    let mut config = webp_rust::LossyEncodingConfig::preset(webp_rust::WebpPreset::Photo);
    config.quality = settings.quality as f32;
    config.method = settings.effort.webp_method();
    config.exact = true;
    config.alpha_quality = 100;
    config.near_lossless = 100;

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        webp_rust::encode_lossy_with_config(&image, &config, None)
    }));
    let output = match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => return Err(OptimizeError::Encode(error.to_string())),
        Err(_) => return Err(OptimizeError::CodecPanic("webp-rust")),
    };
    ensure_output_size(output.len())?;
    Ok(output)
}

fn map_image_format(format: Option<RustImageFormat>) -> Result<ImageFormat, OptimizeError> {
    match format {
        Some(RustImageFormat::Jpeg) => Ok(ImageFormat::Jpeg),
        Some(RustImageFormat::Png) => Ok(ImageFormat::Png),
        Some(RustImageFormat::WebP) => Ok(ImageFormat::WebP),
        _ => Err(OptimizeError::UnsupportedFormat),
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), OptimizeError> {
    if width == 0 || height == 0 {
        return Err(OptimizeError::DimensionsUnsupported);
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(OptimizeError::TooManyPixels {
            pixels: u64::MAX,
            max: MAX_PIXELS,
        })?;
    if pixels > MAX_PIXELS {
        return Err(OptimizeError::TooManyPixels {
            pixels,
            max: MAX_PIXELS,
        });
    }
    Ok(())
}

fn ensure_output_size(bytes: usize) -> Result<(), OptimizeError> {
    if bytes > MAX_OUTPUT_BYTES {
        return Err(OptimizeError::OutputTooLarge {
            bytes,
            max: MAX_OUTPUT_BYTES,
        });
    }
    Ok(())
}

fn detect_animation(format: ImageFormat, bytes: &[u8]) -> bool {
    match format {
        ImageFormat::Jpeg => false,
        ImageFormat::Png => png_has_chunk(bytes, b"acTL"),
        ImageFormat::WebP => webp_has_chunk(bytes, b"ANIM") || webp_has_chunk(bytes, b"ANMF"),
    }
}

fn png_has_chunk(bytes: &[u8], target: &[u8; 4]) -> bool {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.get(..8) != Some(PNG_SIGNATURE) {
        return false;
    }
    let mut offset = 8usize;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let name_start = offset + 4;
        let data_start = offset + 8;
        let Some(next) = data_start
            .checked_add(length)
            .and_then(|value| value.checked_add(4))
        else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        if bytes.get(name_start..name_start + 4) == Some(target) {
            return true;
        }
        if bytes.get(name_start..name_start + 4) == Some(b"IEND") {
            return false;
        }
        offset = next;
    }
    false
}

fn webp_has_chunk(bytes: &[u8], target: &[u8; 4]) -> bool {
    if bytes.len() < 12 || bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WEBP") {
        return false;
    }
    let mut offset = 12usize;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let name = &bytes[offset..offset + 4];
        let length = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let data_start = offset + 8;
        let Some(data_end) = data_start.checked_add(length) else {
            return false;
        };
        if data_end > bytes.len() {
            return false;
        }
        if name == target {
            return true;
        }
        let padded = length + (length & 1);
        let Some(next) = data_start.checked_add(padded) else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        offset = next;
    }
    false
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer as RustImageBuffer, Rgb, Rgba};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dangomushi-image-optimizer-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("data/documents")).expect("create documents");
        fs::create_dir_all(root.join("data/exports")).expect("create exports");
        root
    }

    fn write_png(root: &Path, relative: &Path, alpha: bool) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        let image = if alpha {
            DynamicImage::ImageRgba8(RustImageBuffer::from_fn(8, 8, |x, y| {
                Rgba([
                    (x * 20) as u8,
                    (y * 20) as u8,
                    80,
                    if x == 0 && y == 0 { 0 } else { 255 },
                ])
            }))
        } else {
            DynamicImage::ImageRgb8(RustImageBuffer::from_fn(8, 8, |x, y| {
                Rgb([(x * 20) as u8, (y * 20) as u8, 80])
            }))
        };
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), RustImageFormat::Png)
            .expect("encode fixture");
        fs::write(path, bytes).expect("write fixture");
    }

    #[test]
    fn probes_png_without_extension_trust() {
        let root = temp_workspace("probe");
        let relative = PathBuf::from("data/documents/photo.bin");
        write_png(&root, &relative, true);

        let info = probe_workspace_image(&root, &relative).expect("probe");
        assert_eq!(info.format, ImageFormat::Png);
        assert_eq!((info.width, info.height), (8, 8));
        assert!(info.has_alpha);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn lossless_png_path_round_trips_pixels() {
        let root = temp_workspace("png");
        let relative = PathBuf::from("data/documents/photo.png");
        write_png(&root, &relative, true);
        let original = image::open(root.join(&relative)).expect("decode original").to_rgba8();

        let candidate = optimize_workspace_image(
            &root,
            &relative,
            OptimizeSettings {
                format: OutputFormat::Png,
                ..OptimizeSettings::default()
            },
        )
        .expect("optimize PNG");
        let optimized = image::load_from_memory(&candidate.output_bytes)
            .expect("decode optimized")
            .to_rgba8();

        assert_eq!(candidate.output_format, ImageFormat::Png);
        assert_eq!(original, optimized);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn jpeg_rejects_alpha_without_explicit_flattening() {
        let root = temp_workspace("jpeg-alpha");
        let relative = PathBuf::from("data/documents/alpha.png");
        write_png(&root, &relative, true);

        let error = optimize_workspace_image(
            &root,
            &relative,
            OptimizeSettings {
                format: OutputFormat::Jpeg,
                ..OptimizeSettings::default()
            },
        )
        .expect_err("alpha must be rejected");
        assert!(matches!(error, OptimizeError::AlphaToJpeg));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn jpeg_encoder_produces_decodable_output() {
        let root = temp_workspace("jpeg");
        let relative = PathBuf::from("data/documents/opaque.png");
        write_png(&root, &relative, false);

        let candidate = optimize_workspace_image(
            &root,
            &relative,
            OptimizeSettings {
                format: OutputFormat::Jpeg,
                quality: 84,
                effort: Effort::Balanced,
            },
        )
        .expect("optimize JPEG");

        assert_eq!(&candidate.output_bytes[..2], &[0xff, 0xd8]);
        assert_eq!(candidate.after_preview.dimensions(), (8, 8));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn webp_encoder_round_trips_dimensions_and_alpha() {
        let root = temp_workspace("webp");
        let relative = PathBuf::from("data/documents/alpha.png");
        write_png(&root, &relative, true);

        let candidate = optimize_workspace_image(
            &root,
            &relative,
            OptimizeSettings {
                format: OutputFormat::WebP,
                quality: 88,
                effort: Effort::Balanced,
            },
        )
        .expect("optimize WebP");

        assert_eq!(&candidate.output_bytes[..4], b"RIFF");
        assert_eq!(&candidate.output_bytes[8..12], b"WEBP");
        assert_eq!(candidate.after_preview.dimensions(), (8, 8));
        assert_eq!(candidate.after_preview.to_rgba8().get_pixel(0, 0).0[3], 0);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn export_uses_candidate_bytes_and_never_overwrites() {
        let root = temp_workspace("export");
        let relative = PathBuf::from("data/documents/photo.png");
        write_png(&root, &relative, true);
        let candidate = optimize_workspace_image(&root, &relative, OptimizeSettings::default())
            .expect("candidate");

        let first = export_candidate(&root, &candidate).expect("first export");
        let second = export_candidate(&root, &candidate).expect("second export");

        assert_eq!(first.relative_path, PathBuf::from("data/exports/photo-optimized.png"));
        assert_eq!(second.relative_path, PathBuf::from("data/exports/photo-optimized-2.png"));
        assert_eq!(fs::read(root.join(&first.relative_path)).expect("read export"), candidate.output_bytes);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn export_refuses_stale_source_candidate() {
        let root = temp_workspace("stale");
        let relative = PathBuf::from("data/documents/photo.png");
        write_png(&root, &relative, true);
        let candidate = optimize_workspace_image(&root, &relative, OptimizeSettings::default())
            .expect("candidate");

        fs::write(root.join(&relative), b"changed").expect("mutate source");
        let error = export_candidate(&root, &candidate).expect_err("stale source");
        assert!(matches!(error, OptimizeError::SourceChanged));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn animation_markers_are_detected_without_decoding_frames() {
        let mut fake_png = b"\x89PNG\r\n\x1a\n".to_vec();
        fake_png.extend_from_slice(&8u32.to_be_bytes());
        fake_png.extend_from_slice(b"acTL");
        fake_png.extend_from_slice(&[0u8; 8]);
        fake_png.extend_from_slice(&[0u8; 4]);
        assert!(png_has_chunk(&fake_png, b"acTL"));

        let mut fake_webp = b"RIFF".to_vec();
        fake_webp.extend_from_slice(&16u32.to_le_bytes());
        fake_webp.extend_from_slice(b"WEBP");
        fake_webp.extend_from_slice(b"ANIM");
        fake_webp.extend_from_slice(&0u32.to_le_bytes());
        assert!(webp_has_chunk(&fake_webp, b"ANIM"));
    }

    #[test]
    fn path_traversal_is_rejected() {
        let root = temp_workspace("path");
        let error = probe_workspace_image(&root, Path::new("../outside.png")).expect_err("traversal");
        assert!(matches!(error, OptimizeError::InvalidPath));
        fs::remove_dir_all(root).expect("cleanup");
    }
}
