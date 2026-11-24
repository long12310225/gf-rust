//! Port of `internal/command`: option and argument parsing from the process
//! command line, with environment fallback.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct Parsed {
    args: Vec<String>,
    options: HashMap<String, String>,
}

static STATE: OnceLock<Mutex<Parsed>> = OnceLock::new();

fn state() -> &'static Mutex<Parsed> {
    STATE.get_or_init(|| Mutex::new(Parsed::default()))
}

fn with_state<R>(f: impl FnOnce(&mut Parsed) -> R) -> R {
    let mut guard = state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

/// Matches one argument against `^-{1,2}([0-9A-Za-z_?.\-]+)(=)?(.*)$`, the
/// pattern the Go original compiles. Returns the option name, whether an `=`
/// separator was present, and the remaining text.
///
/// The two-dash form is tried first and the one-dash form second, which is the
/// same leftmost match Go's engine finds for inputs like `--` (name `-`).
fn match_option(arg: &str) -> Option<(&str, bool, &str)> {
    let bytes = arg.as_bytes();
    for dashes in [2usize, 1] {
        if bytes.len() < dashes || !bytes[..dashes].iter().all(|&b| b == b'-') {
            continue;
        }
        let rest = &arg[dashes..];
        let name_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'?' | b'.' | b'-'))
            .count();
        if name_len == 0 {
            continue;
        }
        let name = &rest[..name_len];
        let after = &rest[name_len..];
        if after.contains('\n') {
            return None;
        }
        return match after.strip_prefix('=') {
            Some(value) => Some((name, true, value)),
            None => Some((name, false, after)),
        };
    }
    None
}

/// Parses arguments with the framework's default algorithm.
pub fn parse_using_default_algorithm(args: &[String]) -> (Vec<String>, HashMap<String, String>) {
    let mut parsed_args = Vec::new();
    let mut parsed_options = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let Some((name, has_eq, inline)) = match_option(&args[i]) else {
            parsed_args.push(args[i].clone());
            i += 1;
            continue;
        };
        if has_eq {
            parsed_options.insert(name.to_owned(), inline.to_owned());
        } else if i < args.len() - 1 {
            let next = &args[i + 1];
            if !next.is_empty() && next.as_bytes()[0] == b'-' {
                parsed_options.insert(name.to_owned(), inline.to_owned());
            } else {
                parsed_options.insert(name.to_owned(), next.clone());
                i += 2;
                continue;
            }
        } else {
            parsed_options.insert(name.to_owned(), inline.to_owned());
        }
        i += 1;
    }
    (parsed_args, parsed_options)
}

/// Initializes the package-level parsed arguments.
///
/// With no arguments (or an empty slice) it parses the process arguments once
/// and keeps the previous result afterwards. Any non-empty slice replaces the
/// parsed state.
pub fn init(args: &[String]) {
    if args.is_empty() {
        with_state(|parsed| {
            if parsed.args.is_empty() && parsed.options.is_empty() {
                let os_args: Vec<String> = std::env::args_os()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect();
                let (parsed_args, parsed_options) = parse_using_default_algorithm(&os_args);
                parsed.args = parsed_args;
                parsed.options = parsed_options;
            }
        });
    } else {
        let (parsed_args, parsed_options) = parse_using_default_algorithm(args);
        with_state(|parsed| {
            parsed.args = parsed_args;
            parsed.options = parsed_options;
        });
    }
}

fn ensure_init() {
    init(&[]);
}

/// Returns the option value named `name`, or `default` when it is absent.
pub fn get_opt(name: &str, default: Option<&str>) -> String {
    ensure_init();
    with_state(|parsed| match parsed.options.get(name) {
        Some(value) => value.clone(),
        None => default.unwrap_or_default().to_owned(),
    })
}

/// Returns every parsed option.
pub fn get_opt_all() -> HashMap<String, String> {
    ensure_init();
    with_state(|parsed| parsed.options.clone())
}

/// Returns whether the option named `name` was given.
pub fn contains_opt(name: &str) -> bool {
    ensure_init();
    with_state(|parsed| parsed.options.contains_key(name))
}

/// Returns the argument at `index`, or `default` when it is absent.
pub fn get_arg(index: usize, default: Option<&str>) -> String {
    ensure_init();
    with_state(|parsed| match parsed.args.get(index) {
        Some(value) => value.clone(),
        None => default.unwrap_or_default().to_owned(),
    })
}

/// Returns every parsed argument.
pub fn get_arg_all() -> Vec<String> {
    ensure_init();
    with_state(|parsed| parsed.args.clone())
}

/// Returns the command line option for `key`, falling back to the environment
/// variable with the same name, then to `default`.
///
/// Command line options are lowercased with `_` turned into `.`; environment
/// variables are uppercased with `.` turned into `_`.
pub fn get_opt_with_env(key: &str, default: Option<&str>) -> String {
    let command_key = key.replace('_', ".").to_lowercase();
    if contains_opt(&command_key) {
        return get_opt(&command_key, None);
    }
    let env_key = key.replace('.', "_").to_uppercase();
    if let Ok(value) = std::env::var(&env_key) {
        return value;
    }
    default.unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn default_algorithm_parses_options_and_arguments() {
        let (parsed_args, parsed_options) =
            parse_using_default_algorithm(&args(&["gf", "gen", "-n", "2"]));
        assert_eq!(parsed_args, args(&["gf", "gen"]));
        assert_eq!(parsed_options.get("n").map(String::as_str), Some("2"));

        let (_, parsed_options) =
            parse_using_default_algorithm(&args(&["gf", "gen", "-d", "-n", "1"]));
        assert_eq!(parsed_options.get("d").map(String::as_str), Some(""));
        assert_eq!(parsed_options.get("n").map(String::as_str), Some("1"));

        let (parsed_args, parsed_options) =
            parse_using_default_algorithm(&args(&["gf", "--name=gf", "--v"]));
        assert_eq!(parsed_args, args(&["gf"]));
        assert_eq!(parsed_options.get("name").map(String::as_str), Some("gf"));
        assert_eq!(parsed_options.get("v").map(String::as_str), Some(""));

        let (parsed_args, parsed_options) = parse_using_default_algorithm(&args(&["gf", "-h"]));
        assert_eq!(parsed_args, args(&["gf"]));
        assert_eq!(parsed_options.get("h").map(String::as_str), Some(""));

        let (parsed_args, parsed_options) =
            parse_using_default_algorithm(&args(&["a-1", "b_c", "gf.test"]));
        assert_eq!(parsed_args, args(&["a-1", "b_c", "gf.test"]));
        assert!(parsed_options.is_empty());
    }

    #[test]
    fn readers_see_explicitly_initialized_state() {
        init(&args(&["app", "sub", "-x", "1", "--flag"]));
        assert_eq!(get_arg(0, None), "app");
        assert_eq!(get_arg(1, None), "sub");
        assert_eq!(get_arg(9, Some("fallback")), "fallback");
        assert_eq!(get_arg_all(), args(&["app", "sub"]));
        assert_eq!(get_opt("x", None), "1");
        assert_eq!(get_opt("flag", None), "");
        assert_eq!(get_opt("missing", Some("d")), "d");
        assert!(contains_opt("flag"));
        assert!(!contains_opt("missing"));
        assert_eq!(get_opt_all().len(), 2);
    }

    #[test]
    fn manual_matcher_matches_the_go_regex() {
        let regex = regex::Regex::new(r"^-{1,2}([0-9A-Za-z_?.\-]+)(=)?(.*)$").expect("pattern");
        let mut samples = Vec::new();
        for prefix in ["", "-", "--", "---", "----"] {
            for body in [
                "a", "A_1", "?", "-", "a.b", "a b", "测试", "=x", "", "_", "x-y",
            ] {
                for tail in ["", "=1", "==x", " val", "-1", "\n", "=", "a\nb", "?", " "] {
                    samples.push(format!("{prefix}{body}{tail}"));
                }
            }
        }
        for sample in samples {
            let expected = regex.captures(&sample).map(|captures| {
                (
                    captures
                        .get(1)
                        .map(|m| m.as_str().to_owned())
                        .unwrap_or_default(),
                    captures.get(2).is_some(),
                    captures
                        .get(3)
                        .map(|m| m.as_str().to_owned())
                        .unwrap_or_default(),
                )
            });
            let actual = match_option(&sample)
                .map(|(name, has_eq, rest)| (name.to_owned(), has_eq, rest.to_owned()));
            assert_eq!(actual, expected, "sample {sample:?}");
        }
    }

    #[test]
    fn environment_fallback_uses_upper_snake_case() {
        let expected_path = std::env::var("PATH").unwrap_or_default();
        assert_eq!(get_opt_with_env("path", None), expected_path);
        assert_eq!(get_opt_with_env("gf.test.only.unset", Some("3")), "3");
    }
}
