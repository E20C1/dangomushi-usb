use std::error::Error;
use std::fmt;

pub const DEFAULT_SYMBOL_SET: &str = "!@#$%^&*()-_=+[]{};:,.?";
pub const MIN_PASSWORD_LENGTH: usize = 1;
pub const MAX_PASSWORD_LENGTH: usize = 4096;

const UPPERCASE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWERCASE: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const DIGITS: &[u8] = b"0123456789";
const AMBIGUOUS: &[u8] = b"0O1Il|";
const RANDOM_BUFFER_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorConfig {
    pub length: usize,
    pub uppercase: bool,
    pub lowercase: bool,
    pub digits: bool,
    pub symbols: bool,
    pub symbol_set: String,
    pub require_each_selected_group: bool,
    pub exclude_ambiguous: bool,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            length: 20,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: true,
            symbol_set: DEFAULT_SYMBOL_SET.to_string(),
            require_each_selected_group: true,
            exclude_ambiguous: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomSourceError;

impl fmt::Display for RandomSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("random source unavailable")
    }
}

impl Error for RandomSourceError {}

pub trait RandomSource {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), RandomSourceError>;
}

#[derive(Debug, Default)]
pub struct OsRandom;

impl RandomSource for OsRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), RandomSourceError> {
        getrandom::fill(destination).map_err(|_| RandomSourceError)
    }
}

pub struct GeneratedPassword {
    value: String,
}

impl GeneratedPassword {
    pub fn expose_secret(&self) -> &str {
        &self.value
    }

    pub fn len(&self) -> usize {
        self.value.len()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

impl fmt::Debug for GeneratedPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeneratedPassword([REDACTED])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorError {
    LengthOutOfRange,
    NoCharacterGroups,
    EmptySelectedGroup,
    LengthTooShortForRequiredGroups,
    InvalidSymbolSet,
    RandomSourceUnavailable,
}

impl fmt::Display for GeneratorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::LengthOutOfRange => "password length is outside the supported range",
            Self::NoCharacterGroups => "at least one character group must be selected",
            Self::EmptySelectedGroup => "a selected character group is empty after filtering",
            Self::LengthTooShortForRequiredGroups => {
                "password length is shorter than the number of required character groups"
            }
            Self::InvalidSymbolSet => {
                "symbol set must contain printable ASCII punctuation only"
            }
            Self::RandomSourceUnavailable => "operating-system random source is unavailable",
        };

        formatter.write_str(message)
    }
}

impl Error for GeneratorError {}

pub fn generate(config: &GeneratorConfig) -> Result<GeneratedPassword, GeneratorError> {
    let mut source = OsRandom;
    generate_with_source(config, &mut source)
}

pub fn generate_with_source<R: RandomSource + ?Sized>(
    config: &GeneratorConfig,
    source: &mut R,
) -> Result<GeneratedPassword, GeneratorError> {
    let groups = build_groups(config)?;

    if config.require_each_selected_group && config.length < groups.len() {
        return Err(GeneratorError::LengthTooShortForRequiredGroups);
    }

    let mut combined = Vec::new();
    for group in &groups {
        combined.extend_from_slice(group);
    }

    let mut random = RandomByteStream::new(source);
    let mut output = Vec::with_capacity(config.length);

    if config.require_each_selected_group {
        for group in &groups {
            let index = sample_index(&mut random, group.len())?;
            output.push(group[index]);
        }
    }

    while output.len() < config.length {
        let index = sample_index(&mut random, combined.len())?;
        output.push(combined[index]);
    }

    fisher_yates_shuffle(&mut output, &mut random)?;

    let value = String::from_utf8(output)
        .expect("password generator character groups are restricted to valid ASCII");

    Ok(GeneratedPassword { value })
}

fn build_groups(config: &GeneratorConfig) -> Result<Vec<Vec<u8>>, GeneratorError> {
    if !(MIN_PASSWORD_LENGTH..=MAX_PASSWORD_LENGTH).contains(&config.length) {
        return Err(GeneratorError::LengthOutOfRange);
    }

    let mut groups = Vec::new();

    if config.uppercase {
        groups.push(filtered_group(UPPERCASE, config.exclude_ambiguous));
    }

    if config.lowercase {
        groups.push(filtered_group(LOWERCASE, config.exclude_ambiguous));
    }

    if config.digits {
        groups.push(filtered_group(DIGITS, config.exclude_ambiguous));
    }

    if config.symbols {
        groups.push(symbol_group(
            &config.symbol_set,
            config.exclude_ambiguous,
        )?);
    }

    if groups.is_empty() {
        return Err(GeneratorError::NoCharacterGroups);
    }

    if groups.iter().any(Vec::is_empty) {
        return Err(GeneratorError::EmptySelectedGroup);
    }

    Ok(groups)
}

fn filtered_group(group: &[u8], exclude_ambiguous: bool) -> Vec<u8> {
    group
        .iter()
        .copied()
        .filter(|value| !exclude_ambiguous || !AMBIGUOUS.contains(value))
        .collect()
}

fn symbol_group(symbol_set: &str, exclude_ambiguous: bool) -> Result<Vec<u8>, GeneratorError> {
    if symbol_set.is_empty() {
        return Err(GeneratorError::InvalidSymbolSet);
    }

    let mut output = Vec::new();

    for value in symbol_set.bytes() {
        if !value.is_ascii_punctuation() {
            return Err(GeneratorError::InvalidSymbolSet);
        }

        if exclude_ambiguous && AMBIGUOUS.contains(&value) {
            continue;
        }

        if !output.contains(&value) {
            output.push(value);
        }
    }

    Ok(output)
}

fn fisher_yates_shuffle<R: RandomSource + ?Sized>(
    values: &mut [u8],
    random: &mut RandomByteStream<'_, R>,
) -> Result<(), GeneratorError> {
    for index in (1..values.len()).rev() {
        let swap_with = sample_index(random, index + 1)?;
        values.swap(index, swap_with);
    }

    Ok(())
}

fn sample_index<R: RandomSource + ?Sized>(
    random: &mut RandomByteStream<'_, R>,
    upper_bound: usize,
) -> Result<usize, GeneratorError> {
    debug_assert!(upper_bound > 0);
    debug_assert!(upper_bound <= u32::MAX as usize);

    let bound = upper_bound as u64;
    let source_range = u32::MAX as u64 + 1;
    let acceptance_limit = source_range - (source_range % bound);

    loop {
        let value = u64::from(random.next_u32()?);

        if value < acceptance_limit {
            return Ok((value % bound) as usize);
        }
    }
}

struct RandomByteStream<'a, R: RandomSource + ?Sized> {
    source: &'a mut R,
    buffer: [u8; RANDOM_BUFFER_BYTES],
    position: usize,
}

impl<'a, R: RandomSource + ?Sized> RandomByteStream<'a, R> {
    fn new(source: &'a mut R) -> Self {
        Self {
            source,
            buffer: [0; RANDOM_BUFFER_BYTES],
            position: RANDOM_BUFFER_BYTES,
        }
    }

    fn next_byte(&mut self) -> Result<u8, GeneratorError> {
        if self.position == self.buffer.len() {
            self.source
                .fill_bytes(&mut self.buffer)
                .map_err(|_| GeneratorError::RandomSourceUnavailable)?;
            self.position = 0;
        }

        let value = self.buffer[self.position];
        self.position += 1;
        Ok(value)
    }

    fn next_u32(&mut self) -> Result<u32, GeneratorError> {
        let bytes = [
            self.next_byte()?,
            self.next_byte()?,
            self.next_byte()?,
            self.next_byte()?,
        ];
        Ok(u32::from_le_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CounterSource {
        next: u8,
    }

    impl CounterSource {
        fn new(start: u8) -> Self {
            Self { next: start }
        }
    }

    impl RandomSource for CounterSource {
        fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), RandomSourceError> {
            for byte in destination {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
    }

    struct ScriptedSource;

    impl RandomSource for ScriptedSource {
        fn fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), RandomSourceError> {
            destination.fill(0);
            if destination.len() >= 4 {
                destination[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
            }
            Ok(())
        }
    }

    struct FailingSource;

    impl RandomSource for FailingSource {
        fn fill_bytes(&mut self, _destination: &mut [u8]) -> Result<(), RandomSourceError> {
            Err(RandomSourceError)
        }
    }

    fn all_groups_config() -> GeneratorConfig {
        GeneratorConfig {
            length: 24,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: true,
            symbol_set: DEFAULT_SYMBOL_SET.to_string(),
            require_each_selected_group: true,
            exclude_ambiguous: false,
        }
    }

    #[test]
    fn deterministic_source_generates_requested_length_and_required_groups() {
        let config = all_groups_config();
        let mut source = CounterSource::new(0);

        let generated = generate_with_source(&config, &mut source).expect("generate");
        let value = generated.expose_secret();

        assert_eq!(generated.len(), config.length);
        assert!(value.bytes().any(|byte| byte.is_ascii_uppercase()));
        assert!(value.bytes().any(|byte| byte.is_ascii_lowercase()));
        assert!(value.bytes().any(|byte| byte.is_ascii_digit()));
        assert!(value.bytes().any(|byte| byte.is_ascii_punctuation()));
    }

    #[test]
    fn ambiguous_exclusion_removes_configured_ambiguous_characters() {
        let config = GeneratorConfig {
            length: 96,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: true,
            symbol_set: "|!?".to_string(),
            require_each_selected_group: true,
            exclude_ambiguous: true,
        };
        let mut source = CounterSource::new(19);

        let generated = generate_with_source(&config, &mut source).expect("generate");

        assert!(
            generated
                .expose_secret()
                .bytes()
                .all(|byte| !AMBIGUOUS.contains(&byte))
        );
    }

    #[test]
    fn configurable_symbol_set_limits_symbol_only_generation() {
        let config = GeneratorConfig {
            length: 40,
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: true,
            symbol_set: "~?".to_string(),
            require_each_selected_group: true,
            exclude_ambiguous: false,
        };
        let mut source = CounterSource::new(7);

        let generated = generate_with_source(&config, &mut source).expect("generate");

        assert!(
            generated
                .expose_secret()
                .bytes()
                .all(|byte| matches!(byte, b'~' | b'?'))
        );
    }

    #[test]
    fn rejection_sampling_rejects_biased_tail_value() {
        let mut source = ScriptedSource;
        let mut random = RandomByteStream::new(&mut source);

        // For an upper bound of 10, u32::MAX is in the rejected tail.
        assert_eq!(sample_index(&mut random, 10).expect("sample"), 0);
    }

    #[test]
    fn supports_shuffle_ranges_larger_than_one_byte() {
        let config = GeneratorConfig {
            length: 300,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: false,
            require_each_selected_group: true,
            exclude_ambiguous: true,
            ..GeneratorConfig::default()
        };
        let mut source = CounterSource::new(31);

        let generated = generate_with_source(&config, &mut source).expect("generate");
        assert_eq!(generated.len(), 300);
    }

    #[test]
    fn generated_password_debug_output_is_redacted() {
        let config = GeneratorConfig {
            length: 8,
            uppercase: true,
            lowercase: false,
            digits: false,
            symbols: false,
            symbol_set: DEFAULT_SYMBOL_SET.to_string(),
            require_each_selected_group: false,
            exclude_ambiguous: false,
        };
        let mut source = CounterSource::new(1);

        let generated = generate_with_source(&config, &mut source).expect("generate");

        assert_eq!(format!("{generated:?}"), "GeneratedPassword([REDACTED])");
    }

    #[test]
    fn rejects_invalid_configuration_without_exposing_values() {
        let mut source = CounterSource::new(0);

        let no_groups = GeneratorConfig {
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: false,
            ..GeneratorConfig::default()
        };
        assert_eq!(
            generate_with_source(&no_groups, &mut source).unwrap_err(),
            GeneratorError::NoCharacterGroups
        );

        let too_short = GeneratorConfig {
            length: 2,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: false,
            require_each_selected_group: true,
            ..GeneratorConfig::default()
        };
        assert_eq!(
            generate_with_source(&too_short, &mut source).unwrap_err(),
            GeneratorError::LengthTooShortForRequiredGroups
        );

        let invalid_symbols = GeneratorConfig {
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: true,
            symbol_set: "abc".to_string(),
            ..GeneratorConfig::default()
        };
        assert_eq!(
            generate_with_source(&invalid_symbols, &mut source).unwrap_err(),
            GeneratorError::InvalidSymbolSet
        );

        let filtered_empty_symbols = GeneratorConfig {
            uppercase: false,
            lowercase: false,
            digits: false,
            symbols: true,
            symbol_set: "|".to_string(),
            exclude_ambiguous: true,
            ..GeneratorConfig::default()
        };
        assert_eq!(
            generate_with_source(&filtered_empty_symbols, &mut source).unwrap_err(),
            GeneratorError::EmptySelectedGroup
        );
    }

    #[test]
    fn random_source_failure_is_reduced_to_non_secret_error() {
        let config = GeneratorConfig {
            length: 8,
            uppercase: true,
            lowercase: false,
            digits: false,
            symbols: false,
            require_each_selected_group: false,
            ..GeneratorConfig::default()
        };
        let mut source = FailingSource;

        assert_eq!(
            generate_with_source(&config, &mut source).unwrap_err(),
            GeneratorError::RandomSourceUnavailable
        );
    }
}
