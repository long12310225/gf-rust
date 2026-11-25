//! Port of `os/gcmd`.
//!
//! GoFrame's command package has three layers: the package-level parsed
//! arguments (`Init`/`GetOpt`), a [`Parser`] that understands supported
//! options with names/aliases, and a [`Command`] tree that dispatches
//! sub-commands and prints help.
//!
//! Divergences, recorded in ADR-047:
//!
//! - no `context.Context`: the parser/command no longer travel through
//!   `CtxKey*` values, and callbacks receive only the [`Parser`];
//! - `Command` sub-commands are `Arc<Command>` values; a command stores its
//!   parent path instead of a parent pointer;
//! - the struct-object lane (`NewFromObject`/`AddObject`) is not ported: it
//!   needs `os/gstructs` + `util/gmeta` and the struct-tag decision (ADR-039);
//! - callback panics are not recovered (Go wraps them into
//!   `CodeInternalPanic`); Rust callbacks should return `Err` instead;
//! - `PrintTo(io.Writer)` becomes [`Command::help_text`] returning the text.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gf_container::gvar::Var;
use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_internal::{command, utils};

use crate::gcfg;
use crate::genv;
use gf_text::gstr;
use gf_util::gutil;
use indexmap::IndexMap;

/// The built-in help option name, like `helpOptionName`.
pub const HELP_OPTION_NAME: &str = "help";
/// The built-in help option short name.
pub const HELP_OPTION_NAME_SHORT: &str = "h";
/// The help word-wrap width, like `maxLineChars`.
pub const MAX_LINE_CHARS: usize = 120;

// ---------------------------------------------------------------------------
// Package-level parsed arguments
// ---------------------------------------------------------------------------

/// Parses `args` into the package-level state, like `gcmd.Init`.
pub fn init(args: &[String]) {
    command::init(args);
}

/// Returns the option `name` from the package-level state, like `GetOpt`.
pub fn get_opt(name: &str, default: Option<&str>) -> Option<Var> {
    let value = command::get_opt(name, default);
    if !value.is_empty() {
        return Some(Var::new(Value::String(value)));
    }
    if command::contains_opt(name) {
        return Some(Var::new(Value::String(String::new())));
    }
    None
}

/// Returns every package-level option.
pub fn get_opt_all() -> BTreeMap<String, String> {
    command::get_opt_all().into_iter().collect()
}

/// Returns the argument at `index` from the package-level state.
pub fn get_arg(index: usize, default: Option<&str>) -> Option<Var> {
    let value = command::get_arg(index, default);
    if !value.is_empty() {
        return Some(Var::new(Value::String(value)));
    }
    None
}

/// Returns every package-level argument.
pub fn get_arg_all() -> Vec<String> {
    command::get_arg_all()
}

/// Returns an option, falling back to the environment, like `GetOptWithEnv`.
pub fn get_opt_with_env(key: &str, default: Option<Value>) -> Option<Var> {
    let command_key = utils::format_cmd_key(key);
    if command::contains_opt(&command_key) {
        return Some(Var::new(Value::String(command::get_opt(
            &command_key,
            None,
        ))));
    }
    let environment_key = utils::format_env_key(key);
    if let Some(value) = genv::get(&environment_key, None) {
        return Some(value);
    }
    default.map(Var::new)
}

/// Builds an option string from a map, like `BuildOptions`.
pub fn build_options(options: &BTreeMap<String, String>, prefix: Option<&str>) -> String {
    let lead = prefix.unwrap_or("-");
    let mut result = String::new();
    for (key, value) in options {
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(lead);
        result.push_str(key);
        if !value.is_empty() {
            result.push('=');
            result.push_str(value);
        }
    }
    result
}

/// Prints `info` and reads one line from stdin, like `Scan`.
pub fn scan(info: &str) -> String {
    print!("{info}");
    readline()
}

/// Reads one trimmed line from stdin.
fn readline() -> String {
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    gstr::trim(&line, None)
}

/// Converts a value to its GoFrame string form.
fn gconv_string(value: &Value) -> String {
    gf_util::gconv::string(value)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parsing options, like Go's `ParserOption`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParserOption {
    /// Parses options case-sensitively.
    pub case_sensitive: bool,
    /// Returns an error for an unknown option.
    pub strict: bool,
}

/// The argument parser, like Go's `gcmd.Parser`.
#[derive(Default)]
pub struct Parser {
    option: ParserOption,
    parsed_args: Vec<String>,
    parsed_options: BTreeMap<String, String>,
    passed_options: Vec<(String, bool)>,
    supported_options: BTreeMap<String, bool>,
}

impl Parser {
    /// Parses `args` with the package-level algorithm when no options are
    /// supported, like Go's `Parse`.
    pub fn parse(
        args: &[String],
        supported_options: Option<&[(&str, bool)]>,
        option: ParserOption,
    ) -> Result<Parser, Error> {
        match supported_options {
            None => {
                command::init(args);
                Ok(Parser {
                    parsed_args: get_arg_all(),
                    parsed_options: get_opt_all(),
                    ..Parser::default()
                })
            }
            Some(options) => Parser::parse_args(args, options, option),
        }
    }

    /// Parses `args` with the given supported options, like `ParseArgs`.
    ///
    /// Each entry is `(name[, short], needs_argument)`.
    pub fn parse_args(
        args: &[String],
        supported_options: &[(&str, bool)],
        option: ParserOption,
    ) -> Result<Parser, Error> {
        let mut parser = Parser {
            option,
            parsed_args: Vec::new(),
            parsed_options: BTreeMap::new(),
            passed_options: supported_options
                .iter()
                .map(|(name, needs)| ((*name).to_owned(), *needs))
                .collect(),
            supported_options: BTreeMap::new(),
        };
        for (name, needs_argument) in supported_options {
            for part in name.split(',') {
                parser
                    .supported_options
                    .insert(part.trim().to_owned(), *needs_argument);
            }
        }
        let mut index = 0;
        while index < args.len() {
            let Some(option_name) = parser.parse_option(&args[index]) else {
                parser.parsed_args.push(args[index].clone());
                index += 1;
                continue;
            };
            // `--name=value`.
            if let Some((name, value)) = option_name.split_once('=') {
                if parser.is_option_valid(name) {
                    parser.set_option_value(name, value);
                }
                index += 1;
                continue;
            }
            if parser.is_option_valid(&option_name) {
                if parser.is_option_needs_argument(&option_name) {
                    if index < args.len() - 1 {
                        parser.set_option_value(&option_name, &args[index + 1]);
                        index += 2;
                        continue;
                    }
                } else {
                    parser.set_option_value(&option_name, "");
                    index += 1;
                    continue;
                }
            } else if let Some(names) = parser.parse_multi_option(&option_name) {
                for name in names {
                    parser.set_option_value(&name, "");
                }
                index += 1;
                continue;
            } else if parser.option.strict {
                return Err(gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("invalid option '{}'", args[index]),
                    Vec::new(),
                ));
            }
            index += 1;
        }
        Ok(parser)
    }

    /// Splits a combined short option like `fq` into valid single options.
    fn parse_multi_option(&self, option: &str) -> Option<Vec<String>> {
        let characters: Vec<char> = option.chars().collect();
        for length in 1..=characters.len() {
            let prefix: String = characters[..length].iter().collect();
            if self.is_option_valid(&prefix) && !self.is_option_needs_argument(&prefix) {
                if length == characters.len() {
                    return Some(vec![prefix]);
                }
                let rest: String = characters[length..].iter().collect();
                let mut nested = self.parse_multi_option(&rest)?;
                nested.push(prefix);
                return Some(nested);
            }
        }
        None
    }

    /// Strips the leading dashes, like `parseOption`.
    fn parse_option(&self, argument: &str) -> Option<String> {
        let trimmed = argument
            .strip_prefix("--")
            .or_else(|| argument.strip_prefix('-'))?;
        if trimmed.is_empty() || trimmed.starts_with('-') {
            return None;
        }
        Some(trimmed.to_owned())
    }

    /// Reports whether `name` is a supported option, like `isOptionValid`.
    fn is_option_valid(&self, name: &str) -> bool {
        if self.option.case_sensitive {
            return self.supported_options.contains_key(name);
        }
        self.supported_options
            .keys()
            .any(|option_name| gstr::equal(option_name, name))
    }

    /// Reports whether `name` takes a value, like `isOptionNeedArgument`.
    fn is_option_needs_argument(&self, name: &str) -> bool {
        self.supported_options.get(name).copied().unwrap_or(false)
    }

    /// Stores the value under every alias of `name`, like `setOptionValue`.
    fn set_option_value(&mut self, name: &str, value: &str) {
        for (option_name, _) in &self.passed_options {
            let aliases: Vec<String> = option_name
                .split(',')
                .map(|alias| alias.trim().to_owned())
                .collect();
            if aliases.iter().any(|alias| alias == name) {
                for alias in aliases {
                    self.parsed_options.insert(alias, value.to_owned());
                }
                return;
            }
        }
        for (option_name, _) in &self.passed_options {
            let aliases: Vec<String> = option_name
                .split(',')
                .map(|alias| alias.trim().to_owned())
                .collect();
            if aliases.iter().any(|alias| alias.eq_ignore_ascii_case(name)) {
                for alias in aliases {
                    self.parsed_options.insert(alias, value.to_owned());
                }
                return;
            }
        }
    }

    /// Returns the option `name`, like `GetOpt`.
    pub fn get_opt(&self, name: &str, default: Option<Value>) -> Option<Var> {
        if let Some(value) = self.parsed_options.get(name) {
            return Some(Var::new(Value::String(value.clone())));
        }
        default.map(Var::new)
    }

    /// Returns every parsed option.
    pub fn get_opt_all(&self) -> BTreeMap<String, String> {
        self.parsed_options.clone()
    }

    /// Returns the argument at `index`, like `GetArg`.
    pub fn get_arg(&self, index: usize, default: Option<&str>) -> Option<Var> {
        if let Some(value) = self.parsed_args.get(index) {
            return Some(Var::new(Value::String(value.clone())));
        }
        default.map(|value| Var::new(Value::String(value.to_owned())))
    }

    /// Returns every parsed argument.
    pub fn get_arg_all(&self) -> Vec<String> {
        self.parsed_args.clone()
    }

    /// Reports whether `name` was given (used by `re_parse`).
    fn has_opt(&self, name: &str) -> bool {
        self.parsed_options.contains_key(name)
    }

    /// Encodes the parser as JSON, like `MarshalJSON`.
    pub fn marshal_json(&self) -> Result<String, Error> {
        let mut map = IndexMap::new();
        map.insert(
            "parsedArgs".to_owned(),
            Value::List(
                self.parsed_args
                    .iter()
                    .map(|value| Value::String(value.clone()))
                    .collect(),
            ),
        );
        let mut parsed_options = IndexMap::new();
        for (key, value) in &self.parsed_options {
            parsed_options.insert(key.clone(), Value::String(value.clone()));
        }
        map.insert("parsedOptions".to_owned(), Value::Map(parsed_options));
        let mut passed_options = IndexMap::new();
        for (name, needs) in &self.passed_options {
            passed_options.insert(name.clone(), Value::Bool(*needs));
        }
        map.insert("passedOptions".to_owned(), Value::Map(passed_options));
        let mut supported_options = IndexMap::new();
        for (name, needs) in &self.supported_options {
            supported_options.insert(name.clone(), Value::Bool(*needs));
        }
        map.insert("supportedOptions".to_owned(), Value::Map(supported_options));
        gf_json::marshal_value(&Value::Map(map))
    }
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

/// One command callback, like Go's `Function`.
pub type Function = Arc<dyn Fn(&Parser) -> Result<(), Error> + Send + Sync>;
/// A callback that also returns a value, like Go's `FuncWithValue`.
pub type FuncWithValue = Arc<dyn Fn(&Parser) -> Result<Value, Error> + Send + Sync>;

/// One command argument or option, like Go's `Argument`.
#[derive(Clone, Debug, Default)]
pub struct Argument {
    /// Option/argument name.
    pub name: String,
    /// Option short name.
    pub short: String,
    /// Default value shown in the help text.
    pub default: String,
    /// Help description.
    pub brief: String,
    /// Takes its value from the positional arguments.
    pub is_arg: bool,
    /// Whether the option never takes a value.
    pub orphan: bool,
}

/// The built-in help option, like Go's `defaultHelpOption`.
pub fn default_help_option() -> Argument {
    Argument {
        name: HELP_OPTION_NAME.to_owned(),
        short: HELP_OPTION_NAME_SHORT.to_owned(),
        brief: "more information about this command".to_owned(),
        orphan: true,
        ..Argument::default()
    }
}

/// A console command, like Go's `gcmd.Command`.
pub struct Command {
    /// Command name (case-sensitive).
    pub name: String,
    /// One-line usage, like `gf build main.go [OPTION]`.
    pub usage: String,
    /// Short description.
    pub brief: String,
    /// Detailed description.
    pub description: String,
    /// Configured arguments and options.
    pub arguments: Vec<Argument>,
    /// Custom handler.
    pub run: Option<Function>,
    /// Handler with a return value.
    pub run_with_value: Option<FuncWithValue>,
    /// Custom help handler.
    pub help_func: Option<Function>,
    /// Usage examples.
    pub examples: String,
    /// Extra text appended to the help output.
    pub additional: String,
    /// Returns an error for unknown options.
    pub strict: bool,
    /// Parses options case-sensitively.
    pub case_sensitive: bool,
    /// Config node whose values fill in missing options.
    pub config: String,
    parent_path: Mutex<String>,
    commands: Mutex<Vec<Arc<Command>>>,
}

impl Command {
    /// Creates a command with `name`, like `&gcmd.Command{Name: ...}`.
    pub fn new(name: &str) -> Command {
        Command {
            name: name.to_owned(),
            usage: String::new(),
            brief: String::new(),
            description: String::new(),
            arguments: Vec::new(),
            run: None,
            run_with_value: None,
            help_func: None,
            examples: String::new(),
            additional: String::new(),
            strict: false,
            case_sensitive: false,
            config: String::new(),
            parent_path: Mutex::new(String::new()),
            commands: Mutex::new(Vec::new()),
        }
    }

    /// Adds a sub-command, like Go's `AddCommand`.
    pub fn add_command(&self, command: Arc<Command>) -> Result<(), Error> {
        let name = gstr::trim(&command.name, None);
        if name.is_empty() {
            return Err(gerror::new("command name should not be empty"));
        }
        *command
            .parent_path
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            format!("{}{} ", self.full_name(), name);
        let mut commands = self
            .commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if commands.iter().any(|existing| existing.name == name) {
            return Err(gerror::newf(
                format!(
                    "command \"{name}\" is already added to command \"{}\"",
                    self.name
                ),
                Vec::new(),
            ));
        }
        commands.push(Arc::clone(&command));
        Ok(())
    }

    /// Returns the sub-commands.
    pub fn commands(&self) -> Vec<Arc<Command>> {
        self.commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The full command name including its parents, like Go's parent walk.
    pub fn full_name(&self) -> String {
        format!(
            "{}{}",
            self.parent_path
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            self.name
        )
    }

    /// Runs the command with the process arguments, exiting on error, like
    /// Go's `Run`.
    pub fn run(&self) {
        if let Err(error) = self.run_with_value_error(&std::env::args().collect::<Vec<_>>()) {
            print_error_and_exit(self, &error);
        }
    }

    /// Runs the command and returns the callback value, exiting on error, like
    /// Go's `RunWithValue`.
    pub fn run_with_value(&self) -> Option<Value> {
        match self.run_with_value_error(&std::env::args().collect::<Vec<_>>()) {
            Ok(value) => Some(value),
            Err(error) => {
                print_error_and_exit(self, &error);
            }
        }
    }

    /// Runs the command, like Go's `RunWithError`.
    pub fn run_with_error(&self, args: &[String]) -> Result<(), Error> {
        self.run_with_value_error(args).map(|_| ())
    }

    /// Runs the command and returns the callback value, like
    /// `RunWithValueError`.
    pub fn run_with_value_error(&self, args: &[String]) -> Result<Value, Error> {
        self.run_with_specific_args(args)
    }

    /// Runs the command with specific arguments, like
    /// `RunWithSpecificArgs`.
    pub fn run_with_specific_args(&self, args: &[String]) -> Result<Value, Error> {
        if args.is_empty() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["args can not be empty!"],
            ));
        }
        let parser = Parser::parse_args(args, &[], ParserOption::default())?;
        let mut parsed_args = parser.get_arg_all();
        // Exclude the binary name.
        if !parsed_args.is_empty() {
            parsed_args.remove(0);
        }
        if parsed_args.is_empty() || self.commands().is_empty() {
            return self.do_run(args, &parser);
        }
        match self.search_command(&parsed_args) {
            Some(command) => command.do_run(args, &parser),
            None => Err(gerror::new_codef(
                &gcode::CODE_NOT_FOUND,
                format!(
                    "command \"{}\" not found for command \"{}\", command line: {}",
                    parsed_args.join(" "),
                    self.name,
                    args.join(" ")
                ),
                Vec::new(),
            )),
        }
    }

    /// Finds the sub-command matching `args[0]`, like `searchCommand`.
    fn search_command(&self, args: &[String]) -> Option<Arc<Command>> {
        let first = args.first()?;
        for command in self.commands() {
            if command.name != *first {
                continue;
            }
            let left = &args[1..];
            if command.has_argument_from_index() || left.is_empty() {
                return Some(command);
            }
            return command.search_command(left);
        }
        None
    }

    /// Runs this command with `parser`, like `doRun`.
    fn do_run(&self, args: &[String], parser: &Parser) -> Result<Value, Error> {
        if parser.get_opt(HELP_OPTION_NAME, None).is_some()
            || parser.get_opt(HELP_OPTION_NAME_SHORT, None).is_some()
        {
            if let Some(help) = &self.help_func {
                return help(parser).map(|_| Value::Null);
            }
            self.print();
            return Ok(Value::Null);
        }
        let parser = self.re_parse(args, parser)?;
        if let Some(run) = &self.run {
            run(&parser)?;
            return Ok(Value::Null);
        }
        if let Some(run) = &self.run_with_value {
            return run(&parser);
        }
        if let Some(help) = &self.help_func {
            return help(&parser).map(|_| Value::Null);
        }
        self.print();
        Ok(Value::Null)
    }

    /// Re-parses `args` with this command's options, like `reParse`.
    fn re_parse(&self, args: &[String], parser: &Parser) -> Result<Parser, Error> {
        if self.arguments.is_empty() {
            return Ok(Parser {
                parsed_args: parser.parsed_args.clone(),
                parsed_options: parser.parsed_options.clone(),
                ..Parser::default()
            });
        }
        let mut supported: Vec<(String, bool)> = Vec::new();
        for argument in &self.arguments {
            if argument.is_arg {
                continue;
            }
            let key = if argument.short.is_empty() {
                argument.name.clone()
            } else {
                format!("{},{}", argument.name, argument.short)
            };
            supported.push((key, !argument.orphan));
        }
        let options: Vec<(&str, bool)> = supported
            .iter()
            .map(|(key, needs)| (key.as_str(), *needs))
            .collect();
        let mut re_parsed = Parser::parse_args(
            args,
            &options,
            ParserOption {
                case_sensitive: self.case_sensitive,
                strict: self.strict,
            },
        )?;
        // Fill missing options from the config node.
        if !self.config.is_empty()
            && let Some(config) = gcfg::instance(None)
            && config.available(None)
            && let Some(value) = config.get(&self.config)?
        {
            let config_map = value.map();
            for option_name in re_parsed.supported_options.keys() {
                if re_parsed.has_opt(option_name) {
                    continue;
                }
                if let Some((_, found_value)) =
                    gutil::map_possible_item_by_key(&config_map, option_name)
                {
                    re_parsed
                        .parsed_options
                        .insert(option_name.clone(), gconv_string(found_value));
                }
            }
        }
        Ok(re_parsed)
    }

    /// Reports whether an argument takes its value positionally.
    fn has_argument_from_index(&self) -> bool {
        self.arguments.iter().any(|argument| argument.is_arg)
    }

    /// Reports whether an option takes a value.
    fn has_argument_from_option(&self) -> bool {
        self.arguments.iter().any(|argument| !argument.is_arg)
    }

    /// Prints the help text to stdout, like Go's `Print`.
    pub fn print(&self) {
        print!("{}", self.help_text());
    }

    /// Renders the help text, like Go's `PrintTo`.
    pub fn help_text(&self) -> String {
        let prefix = "    ".to_owned();
        let mut arguments = self.arguments.clone();
        arguments.push(default_help_option());
        let mut buffer = String::new();

        // USAGE.
        if !self.usage.is_empty() || !self.name.is_empty() {
            buffer.push_str("USAGE\n");
            buffer.push_str(&prefix);
            if !self.usage.is_empty() {
                buffer.push_str(&self.usage);
            } else {
                buffer.push_str(&self.full_name());
                if !self.commands().is_empty() {
                    buffer.push_str(" COMMAND");
                }
                if self.has_argument_from_index() {
                    buffer.push_str(" ARGUMENT");
                }
                buffer.push_str(" [OPTION]");
            }
            buffer.push_str("\n\n");
        }
        // COMMAND.
        let commands = self.commands();
        if !commands.is_empty() {
            buffer.push_str("COMMAND\n");
            let max_name = commands
                .iter()
                .map(|command| command.name.chars().count())
                .max()
                .unwrap_or(0);
            for command in &commands {
                let space_length = max_name - command.name.chars().count();
                let wordwrap_prefix = " ".repeat(prefix.chars().count() + max_name + 4);
                print_line_brief(
                    &mut buffer,
                    &command.name,
                    &prefix,
                    &gstr::trim(&command.brief, None),
                    &wordwrap_prefix,
                    space_length,
                );
            }
            buffer.push('\n');
        }
        // ARGUMENT.
        if self.has_argument_from_index() {
            buffer.push_str("ARGUMENT\n");
            let max_name = arguments
                .iter()
                .filter(|argument| argument.is_arg)
                .map(|argument| argument.name.chars().count())
                .max()
                .unwrap_or(0);
            for argument in arguments.iter().filter(|argument| argument.is_arg) {
                let space_length = max_name - argument.name.chars().count();
                let wordwrap_prefix = " ".repeat(prefix.chars().count() + max_name + 4);
                print_line_brief(
                    &mut buffer,
                    &argument.name,
                    &prefix,
                    &gstr::trim(&argument.brief, None),
                    &wordwrap_prefix,
                    space_length,
                );
            }
            buffer.push('\n');
        }
        // OPTION.
        if self.has_argument_from_option() {
            buffer.push_str("OPTION\n");
            let name_of = |argument: &Argument| {
                if argument.short.is_empty() {
                    format!("-/--{}", argument.name)
                } else {
                    format!("-{}, --{}", argument.short, argument.name)
                }
            };
            let max_name = arguments
                .iter()
                .filter(|argument| !argument.is_arg)
                .map(|argument| name_of(argument).chars().count())
                .max()
                .unwrap_or(0);
            for argument in arguments.iter().filter(|argument| !argument.is_arg) {
                let name = name_of(argument);
                let space_length = max_name - name.chars().count();
                let wordwrap_prefix = " ".repeat(prefix.chars().count() + max_name + 4);
                let mut brief = gstr::trim(&argument.brief, None);
                if !argument.default.is_empty() {
                    brief = format!("{brief} (default: \"{}\")", argument.default);
                }
                print_line_brief(
                    &mut buffer,
                    &name,
                    &prefix,
                    &brief,
                    &wordwrap_prefix,
                    space_length,
                );
            }
            buffer.push('\n');
        }
        // EXAMPLE.
        if !self.examples.is_empty() {
            buffer.push_str("EXAMPLE\n");
            for line in gstr::split_and_trim(&self.examples, "\n", None) {
                buffer.push_str(&prefix);
                buffer.push_str(&word_wrap(&line, MAX_LINE_CHARS, &format!("\n{prefix}")));
                buffer.push('\n');
            }
            buffer.push('\n');
        }
        // DESCRIPTION.
        if !self.description.is_empty() {
            buffer.push_str("DESCRIPTION\n");
            for line in gstr::split_and_trim(&self.description, "\n", None) {
                buffer.push_str(&prefix);
                buffer.push_str(&word_wrap(&line, MAX_LINE_CHARS, &format!("\n{prefix}")));
                buffer.push('\n');
            }
            buffer.push('\n');
        }
        // ADDITIONAL.
        if !self.additional.is_empty() {
            buffer.push_str(&word_wrap(&self.additional, MAX_LINE_CHARS, "\n"));
            buffer.push('\n');
        }
        buffer.replace('\t', "    ")
    }
}

/// Prints one name plus its brief, wrapping long lines like Go.
fn print_line_brief(
    buffer: &mut String,
    name: &str,
    prefix: &str,
    brief: &str,
    wordwrap_prefix: &str,
    space_length: usize,
) {
    let mut lines = gstr::split_and_trim(brief, "\n", None);
    if lines.is_empty() {
        lines.push(String::new());
    }
    for (index, line) in lines.iter().enumerate() {
        let line_str = if index == 0 {
            format!("{prefix}{name}{}{}\n", " ".repeat(space_length + 4), line)
        } else {
            format!(
                "{prefix}{}{}{}\n",
                " ".repeat(name.chars().count()),
                " ".repeat(space_length + 4),
                line
            )
        };
        buffer.push_str(&word_wrap(
            &line_str,
            MAX_LINE_CHARS,
            &format!("\n{wordwrap_prefix}"),
        ));
    }
}

/// Wraps `text` at spaces, like GoFrame's `gstr.WordWrap`.
fn word_wrap(text: &str, width: usize, newline_prefix: &str) -> String {
    let mut output = String::new();
    for (line_index, line) in text.split('\n').enumerate() {
        if line_index > 0 {
            output.push_str(newline_prefix);
        }
        let mut current = 0usize;
        for (word_index, word) in line.split(' ').enumerate() {
            let word_length = word.chars().count();
            if word_index > 0 {
                if current > 0 && current + 1 + word_length > width {
                    output.push('\n');
                    output.push_str(newline_prefix);
                    current = 0;
                } else {
                    output.push(' ');
                    current += 1;
                }
            }
            output.push_str(word);
            current += word_length;
        }
    }
    output
}

/// Prints the error and exits with status 1, like `RunWithValue`.
fn print_error_and_exit(command: &Command, error: &Error) -> ! {
    let text = error.to_string();
    if gerror::code(error) == gcode::CODE_NOT_FOUND {
        eprintln!("ERROR: {}", gstr::trim(&text, None));
    } else {
        eprintln!("{error:?}");
    }
    command.print();
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    // Translated from Test_ParseArgs.
    #[test]
    fn parser_parses_options_and_arguments() {
        let args: Vec<String> = [
            "gf", "--force", "remove", "-fq", "-p=www", "path", "-n", "root",
        ]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
        let parser = Parser::parse_args(
            &args,
            &[
                ("n, name", true),
                ("p, prefix", true),
                ("f,force", false),
                ("q,quiet", false),
            ],
            ParserOption::default(),
        )
        .expect("parse");

        assert_eq!(parser.get_arg_all(), vec!["gf", "remove", "path"]);
        assert_eq!(parser.get_arg(0, None).expect("arg0").string(), "gf");
        assert_eq!(parser.get_arg(1, None).expect("arg1").string(), "remove");
        assert_eq!(parser.get_arg(2, None).expect("arg2").string(), "path");

        assert_eq!(parser.get_opt_all().len(), 8);
        assert_eq!(parser.get_opt("n", None).expect("n").string(), "root");
        assert_eq!(parser.get_opt("name", None).expect("name").string(), "root");
        assert_eq!(parser.get_opt("p", None).expect("p").string(), "www");
        assert_eq!(
            parser.get_opt("prefix", None).expect("prefix").string(),
            "www"
        );
        assert!(parser.get_opt("f", None).is_some());
        assert!(parser.get_opt("force", None).is_some());
        assert!(parser.get_opt("q", None).is_some());
        assert!(parser.get_opt("quiet", None).is_some());
        assert!(parser.get_opt("none", None).is_none());
        assert_eq!(
            parser
                .get_opt("none", Some(Value::String("value".to_owned())))
                .expect("default")
                .string(),
            "value"
        );
        assert!(!parser.marshal_json().expect("json").is_empty());
    }

    // Translated from Test_Parse's package-state branch.
    #[test]
    fn parser_without_supported_options_uses_package_state() {
        let args: Vec<String> = [
            "gf", "--force", "remove", "-fq", "-p=www", "path", "-n", "root",
        ]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
        let _guard = crate::test_support::command_lock();
        let parser = Parser::parse(&args, None, ParserOption::default()).expect("parse");
        let arguments = parser.get_arg_all();
        assert_eq!(arguments, vec!["gf", "path"]);
        assert_eq!(
            parser.get_opt("force", None).expect("force").string(),
            "remove"
        );
        assert_eq!(parser.get_opt("n", None).expect("n").string(), "root");
        assert!(parser.get_opt("fq", None).is_some());
        assert!(parser.get_opt("p", None).is_some());
        assert!(parser.get_opt("none", None).is_none());
    }

    // Translated from Test_Default.
    #[test]
    fn package_level_init_and_getters() {
        let args: Vec<String> = [
            "gf", "--force", "remove", "-fq", "-p=www", "path", "-n", "root",
        ]
        .iter()
        .map(|item| (*item).to_owned())
        .collect();
        let _guard = crate::test_support::command_lock();
        init(&args);
        assert_eq!(get_arg_all().len(), 2);
        assert_eq!(get_arg(1, None).expect("arg").string(), "path");
        assert_eq!(
            get_arg(100, Some("test")).expect("default").string(),
            "test"
        );
        assert_eq!(get_opt("force", None).expect("force").string(), "remove");
        assert_eq!(get_opt("n", None).expect("n").string(), "root");
        assert!(get_opt("fq", None).is_some());
        assert!(get_opt("p", None).is_some());
        assert!(get_opt("none", None).is_none());
        assert_eq!(
            get_opt("none", Some("value")).expect("default").string(),
            "value"
        );

        init(&["gf".to_owned(), "gen".to_owned(), "-h".to_owned()]);
        assert_eq!(get_arg_all().len(), 2);
        assert_eq!(get_opt("h", None).expect("h").string(), "");
    }

    // Translated from Test_BuildOptions.
    #[test]
    fn build_options_renders_the_map() {
        let mut options = BTreeMap::new();
        options.insert("n".to_owned(), "john".to_owned());
        assert_eq!(build_options(&options, None), "-n=john");
        assert_eq!(build_options(&options, Some("-test")), "-testn=john");

        let mut multiple = BTreeMap::new();
        multiple.insert("n1".to_owned(), "john".to_owned());
        multiple.insert("n2".to_owned(), "huang".to_owned());
        let rendered = build_options(&multiple, None);
        assert!(rendered.contains("-n1=john"));
        assert!(rendered.contains("-n2=huang"));
    }

    // Translated from Test_GetWithEnv.
    #[test]
    fn get_opt_with_env_falls_back_to_environment() {
        genv::set("TESTGCMDENVUNIQUE", "1").expect("set env");
        let value = get_opt_with_env("testgcmdenvunique", None).expect("env value");
        assert_eq!(value.string(), "1");
        genv::remove(&["TESTGCMDENVUNIQUE"]).expect("remove env");

        // A given command-line option wins over the environment.
        let _guard_commands = crate::test_support::command_lock();
        init(&["-anotheropt".to_owned(), "2".to_owned()]);
        let value = get_opt_with_env("anotheropt", None).expect("option value");
        assert_eq!(value.string(), "2");
    }

    // Translated from Test_Command: the sub-command runs with its own options.
    #[test]
    fn command_dispatches_sub_commands() {
        let root = Arc::new(Command::new("gf"));

        let called = Arc::new(AtomicBool::new(false));
        let env_called = Arc::clone(&called);
        let mut env = Command::new("env");
        env.run = Some(Arc::new(move |_parser: &Parser| {
            env_called.store(true, Ordering::SeqCst);
            Ok(())
        }));
        root.add_command(Arc::new(env)).expect("add env");

        let caught = Arc::new(Mutex::new(None));
        let test_caught = Arc::clone(&caught);
        let mut test = Command::new("test");
        test.arguments = vec![
            Argument {
                name: "my-option".to_owned(),
                short: "o".to_owned(),
                brief: "custom option".to_owned(),
                orphan: true,
                ..Argument::default()
            },
            Argument {
                name: "name".to_owned(),
                short: "n".to_owned(),
                brief: "name option".to_owned(),
                ..Argument::default()
            },
            Argument {
                name: "arg1".to_owned(),
                brief: "first argument".to_owned(),
                is_arg: true,
                ..Argument::default()
            },
        ];
        test.run = Some(Arc::new(move |parser: &Parser| {
            let mut slot = test_caught
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *slot = Some((
                parser
                    .get_opt("my-option", None)
                    .map(|value| value.string()),
                parser.get_opt("name", None).map(|value| value.string()),
                parser.get_arg(2, None).map(|value| value.string()),
            ));
            Ok(())
        }));
        root.add_command(Arc::new(test)).expect("add test");

        root.run_with_specific_args(&["gf".to_owned(), "env".to_owned()])
            .expect("run env");
        assert!(called.load(Ordering::SeqCst));

        root.run_with_specific_args(&[
            "gf".to_owned(),
            "test".to_owned(),
            "-o".to_owned(),
            "--name".to_owned(),
            "john".to_owned(),
            "arg1".to_owned(),
        ])
        .expect("run test");
        let captured = caught
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .expect("captured");
        assert_eq!(captured.0.as_deref(), Some(""));
        assert_eq!(captured.1.as_deref(), Some("john"));
        assert_eq!(captured.2.as_deref(), Some("arg1"));

        // Unknown commands report CodeNotFound.
        let error = root
            .run_with_specific_args(&["gf".to_owned(), "unknown".to_owned()])
            .expect_err("unknown command");
        assert_eq!(gerror::code(&error), gcode::CODE_NOT_FOUND);

        // The help option prints instead of running.
        root.run_with_specific_args(&["gf".to_owned(), "test".to_owned(), "-h".to_owned()])
            .expect("help");
    }

    #[test]
    fn add_command_validates_names() {
        let root = Command::new("root");
        let empty = Arc::new(Command::new(" "));
        assert!(root.add_command(empty).is_err());

        root.add_command(Arc::new(Command::new("sub")))
            .expect("add");
        let duplicate = Arc::new(Command::new("sub"));
        let error = root.add_command(duplicate).expect_err("duplicate");
        assert!(error.to_string().contains("already added"));
    }

    #[test]
    fn help_text_contains_every_section() {
        let root = Command::new("gf");
        let mut sub = Command::new("test");
        sub.brief = "test brief".to_owned();
        sub.description = "detailed description".to_owned();
        sub.examples = "gf test -n john".to_owned();
        sub.additional = "more info".to_owned();
        sub.arguments = vec![
            Argument {
                name: "name".to_owned(),
                short: "n".to_owned(),
                brief: "the name".to_owned(),
                default: "john".to_owned(),
                ..Argument::default()
            },
            Argument {
                name: "arg1".to_owned(),
                brief: "first argument".to_owned(),
                is_arg: true,
                ..Argument::default()
            },
        ];
        root.add_command(Arc::new(sub)).expect("add");

        let text = root.help_text();
        assert!(text.contains("USAGE"), "got {text}");
        assert!(text.contains("COMMAND"), "got {text}");
        assert!(text.contains("test brief"), "got {text}");

        let sub = root.commands().into_iter().next().expect("sub");
        let sub_text = sub.help_text();
        assert!(sub_text.contains("ARGUMENT"), "got {sub_text}");
        assert!(sub_text.contains("OPTION"), "got {sub_text}");
        assert!(sub_text.contains("--help"), "got {sub_text}");
        assert!(sub_text.contains("EXAMPLE"), "got {sub_text}");
        assert!(sub_text.contains("DESCRIPTION"), "got {sub_text}");
        assert!(sub_text.contains("more info"), "got {sub_text}");
        assert!(sub_text.contains("(default: \"john\")"), "got {sub_text}");
    }

    #[test]
    fn word_wrap_breaks_long_lines() {
        let text = word_wrap("one two three four five six seven eight", 12, "\n    ");
        assert!(text.contains('\n'), "got {text}");
        for line in text.split('\n') {
            assert!(line.chars().count() <= 12 + 4, "line too long: {line}");
        }
    }
}
