//! Port of `os/gview` — a template engine over [`Value`] data.
//!
//! GoFrame's `gview` is a thin layer over Go's `text/template` (and
//! `html/template` when `AutoEncode` is enabled): it resolves template files
//! across search paths and the resource manager, caches them, and provides a
//! set of built-in functions. Rust has no Go template engine, so this port
//! implements the commonly used subset directly (ADR-058):
//!
//! - actions `{{...}}`, comments `{{/* ... */}}`, whitespace trim markers
//!   `{{- ... -}}`, configurable delimiters;
//! - field paths (`.name`, `.user.name`), the root dot, and variables
//!   (`$x`, `$x.name`);
//! - pipelines (`{{"x" | html}}`) with the previous result appended as the
//!   last argument, like Go;
//! - `if` / `else if` / `else` / `end`, `range` (lists, maps and integers,
//!   with optional `$k, $v :=`), `with`;
//! - built-in functions: comparisons, text/html/url codecs, `date`, `substr`,
//!   `strlimit`, `concat`, `replace`, `compare`, `hidestr`, `highlight`,
//!   `toupper`, `tolower`, `nl2br`, `include`, `map`, `maps`, `json`, `xml`,
//!   `ini`, `yaml`, `yamli`, `toml`, `plus`, `minus`, `times`, `divide`;
//! - file search across paths, template sub-folders and the resource manager,
//!   with mtime-based cache refresh (hot reload) and `AutoEncode` escaping.
//!
//! Not ported (documented in ADR-058): Go `define`/`template`/`block`
//! definitions, method calls on Go structs, `printf`-style formatting and Go
//! `html/template` context-aware escaping; i18n result translation is wired
//! through [`View::set_i18n`] (ADR-088).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::SystemTime;

use gf_container::gvar::Var;
use gf_core::Value;
use gf_encoding::{ghtml, gjson, gurl};
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_i18n::gi18n;
use gf_text::gstr;
use gf_util::gconv;
use indexmap::IndexMap;

use crate::{gfile, gres, gtime};

/// Template parameters, like Go's `Params`.
pub type Params = IndexMap<String, Value>;

/// A template function, like an entry of Go's `FuncMap`.
pub type FuncValue = Arc<dyn Fn(&[Value]) -> Result<Value, Error> + Send + Sync>;

/// A function map, like Go's `FuncMap`.
pub type FuncMap = HashMap<String, FuncValue>;

/// The default template file name, like Go's `defaultParsingFile`.
pub const DEFAULT_PARSING_FILE: &str = "index.html";

/// The template engine configuration, like Go's `Config`.
#[derive(Clone, Debug)]
pub struct Config {
    /// Template search paths.
    pub paths: Vec<String>,
    /// Global template variables.
    pub data: Params,
    /// Default template file for [`View::parse_default`].
    pub default_file: String,
    /// Custom delimiters.
    pub delimiters: (String, String),
    /// Automatically HTML-encode interpolated values.
    pub auto_encode: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            paths: Vec::new(),
            data: IndexMap::new(),
            default_file: DEFAULT_PARSING_FILE.to_owned(),
            delimiters: ("{{".to_owned(), "}}".to_owned()),
            auto_encode: false,
        }
    }
}

/// The options for one parse call, like Go's `Options`.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Template file path.
    pub file: String,
    /// Template content; it wins over `file` when non-empty.
    pub content: String,
    /// Parse `file` alone instead of its folder.
    pub orphan: bool,
    /// Template parameters.
    pub params: Option<Params>,
}

/// One cached template file.
struct CacheItem {
    path: String,
    content: String,
    modified: Option<SystemTime>,
}

/// The shared view state.
struct ViewInner {
    paths: RwLock<Vec<String>>,
    data: RwLock<Params>,
    funcs: RwLock<FuncMap>,
    config: RwLock<Config>,
    cache: Mutex<HashMap<String, CacheItem>>,
    i18n: RwLock<Option<Arc<gi18n::Manager>>>,
}

/// The template engine, like Go's `View`.
#[derive(Clone)]
pub struct View {
    inner: Arc<ViewInner>,
}

impl Default for View {
    fn default() -> View {
        View::new()
    }
}

impl View {
    /// Creates a view whose first search path is the current working
    /// directory, like `New` without a path.
    pub fn new() -> View {
        let view = View {
            inner: Arc::new(ViewInner {
                paths: RwLock::new(Vec::new()),
                data: RwLock::new(IndexMap::new()),
                funcs: RwLock::new(FuncMap::new()),
                config: RwLock::new(Config::default()),
                cache: Mutex::new(HashMap::new()),
                i18n: RwLock::new(None),
            }),
        };
        let pwd = gfile::pwd();
        if !pwd.is_empty() && gfile::is_dir(&pwd) {
            let _ = view.set_path(&pwd);
        }
        view
    }

    /// Creates a view with `path` as its search path, like `New(path)`.
    pub fn with_path(path: &str) -> View {
        let view = View::new();
        let _ = view.set_path(path);
        view
    }

    /// Returns the default view, like `Instance`.
    pub fn instance() -> &'static View {
        static INSTANCE: OnceLock<View> = OnceLock::new();
        INSTANCE.get_or_init(View::new)
    }

    /// Binds an i18n manager, like `SetI18n`; rendered results are then
    /// translated (`I18nLanguage` in the params overrides the manager
    /// language).
    pub fn set_i18n(&self, manager: Arc<gi18n::Manager>) {
        *self
            .inner
            .i18n
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(manager);
    }

    /// The bound i18n manager, if any.
    pub fn i18n(&self) -> Option<Arc<gi18n::Manager>> {
        self.inner
            .i18n
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Translates a rendered result through the bound manager.
    fn apply_i18n(&self, content: String, params: Option<&Params>) -> String {
        let Some(manager) = self.i18n() else {
            return content;
        };
        let language = params
            .and_then(|params| params.get("I18nLanguage"))
            .map(gconv::string);
        match language {
            Some(language) => manager.translate_with_language(&content, &language),
            None => manager.translate(&content),
        }
    }

    /// Replaces the search paths with `path`, like `SetPath`.
    pub fn set_path(&self, path: &str) -> Result<(), Error> {
        let real = self.resolve_path(path)?;
        let mut paths = self
            .inner
            .paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        paths.clear();
        paths.push(real);
        self.clear_cache();
        Ok(())
    }

    /// Adds `path` to the search paths, like `AddPath`.
    pub fn add_path(&self, path: &str) -> Result<(), Error> {
        let real = self.resolve_path(path)?;
        let mut paths = self
            .inner
            .paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !paths.contains(&real) {
            paths.push(real);
            drop(paths);
            self.clear_cache();
        }
        Ok(())
    }

    /// Resolves and validates a search path.
    fn resolve_path(&self, path: &str) -> Result<String, Error> {
        let mut real = gfile::real_path(path);
        if real.is_empty() {
            let paths = self
                .inner
                .paths
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            for base in paths {
                let candidate = gfile::join(&[&base, path]);
                if gfile::exists(&candidate) {
                    real = gfile::real_path(&candidate);
                    break;
                }
            }
        }
        if real.is_empty() {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("path \"{path}\" does not exist"),
                Vec::new(),
            ));
        }
        if !gfile::is_dir(&real) {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("path \"{path}\" should be directory type"),
                Vec::new(),
            ));
        }
        Ok(real)
    }

    /// The current search paths.
    pub fn paths(&self) -> Vec<String> {
        self.inner
            .paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Assigns one global variable, like `Assign`.
    pub fn assign(&self, key: &str, value: Value) {
        self.inner
            .data
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.to_owned(), value);
    }

    /// Assigns several global variables, like `Assigns`.
    pub fn assigns(&self, data: Params) {
        let mut current = self
            .inner
            .data
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (key, value) in data {
            current.insert(key, value);
        }
    }

    /// Removes every global variable, like `ClearAssigns`.
    pub fn clear_assigns(&self) {
        self.inner
            .data
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Sets the default template file, like `SetDefaultFile`.
    pub fn set_default_file(&self, file: &str) {
        self.inner
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .default_file = file.to_owned();
    }

    /// Returns the default template file, like `GetDefaultFile`.
    pub fn default_file(&self) -> String {
        self.inner
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .default_file
            .clone()
    }

    /// Sets the delimiters, like `SetDelimiters`.
    pub fn set_delimiters(&self, left: &str, right: &str) {
        self.inner
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .delimiters = (left.to_owned(), right.to_owned());
    }

    /// Enables or disables automatic HTML encoding, like `SetAutoEncode`.
    pub fn set_auto_encode(&self, enabled: bool) {
        self.inner
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .auto_encode = enabled;
    }

    /// Binds one template function, like `BindFunc`.
    pub fn bind_func(&self, name: &str, function: FuncValue) {
        self.inner
            .funcs
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned(), function);
    }

    /// Binds several template functions, like `BindFuncMap`.
    pub fn bind_func_map(&self, funcs: FuncMap) {
        let mut current = self
            .inner
            .funcs
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (name, function) in funcs {
            current.insert(name, function);
        }
    }

    /// Applies a configuration, like `SetConfig`.
    pub fn set_config(&self, config: Config) -> Result<(), Error> {
        for path in &config.paths {
            self.add_path(path)?;
        }
        if !config.data.is_empty() {
            self.assigns(config.data.clone());
        }
        if !config.default_file.is_empty() {
            self.set_default_file(&config.default_file);
        }
        if !config.delimiters.0.is_empty() && !config.delimiters.1.is_empty() {
            self.set_delimiters(&config.delimiters.0, &config.delimiters.1);
        }
        let mut current = self
            .inner
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = Config {
            paths: config.paths,
            data: config.data.clone(),
            default_file: if config.default_file.is_empty() {
                DEFAULT_PARSING_FILE.to_owned()
            } else {
                config.default_file
            },
            delimiters: config.delimiters,
            auto_encode: config.auto_encode,
        };
        drop(current);
        self.clear_cache();
        Ok(())
    }

    /// Applies a configuration from a map, like `SetConfigWithMap`.
    pub fn set_config_with_map(&self, map: &Params) -> Result<(), Error> {
        if map.is_empty() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["configuration cannot be empty"],
            ));
        }
        let mut config = Config::default();
        let mut paths: Vec<String> = Vec::new();
        for (key, value) in map {
            match key.to_lowercase().as_str() {
                "paths" | "path" => match value {
                    Value::String(path) => paths.push(path.clone()),
                    Value::List(items) => {
                        paths.extend(items.iter().map(gconv::string));
                    }
                    _ => {}
                },
                "data" => {
                    if let Value::Map(data) = value {
                        config.data = data.clone();
                    }
                }
                "defaultfile" | "default_file" => {
                    config.default_file = gconv::string(value);
                }
                "delimiters" => {
                    if let Value::List(items) = value
                        && items.len() >= 2
                    {
                        config.delimiters = (gconv::string(&items[0]), gconv::string(&items[1]));
                    }
                }
                "autoencode" | "auto_encode" => {
                    config.auto_encode = Var::new(value.clone()).bool();
                }
                _ => {}
            }
        }
        config.paths = paths;
        self.set_config(config)
    }

    fn clear_cache(&self) {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Parses a template file with `params`, like `Parse`.
    pub fn parse(&self, file: &str, params: Option<&Params>) -> Result<String, Error> {
        self.parse_with_options(Options {
            file: file.to_owned(),
            params: params.cloned(),
            ..Options::default()
        })
    }

    /// Parses the default template file, like `ParseDefault`.
    pub fn parse_default(&self, params: Option<&Params>) -> Result<String, Error> {
        let file = self.default_file();
        self.parse(&file, params)
    }

    /// Parses template content, like `ParseContent`.
    pub fn parse_content(&self, content: &str, params: Option<&Params>) -> Result<String, Error> {
        self.parse_with_options(Options {
            content: content.to_owned(),
            params: params.cloned(),
            ..Options::default()
        })
    }

    /// Parses the default template file with content semantics, like
    /// `ParseContentDefault` when the file is the default one.
    pub fn parse_content_default(&self, params: Option<&Params>) -> Result<String, Error> {
        self.parse_default(params)
    }

    /// Parses with [`Options`].
    pub fn parse_with_options(&self, options: Options) -> Result<String, Error> {
        if !options.content.is_empty() {
            let rendered = self.render(&options.content, options.params.as_ref(), 0)?;
            return Ok(self.apply_i18n(rendered, options.params.as_ref()));
        }
        if options.file.is_empty() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["template file cannot be empty"],
            ));
        }
        if options.orphan {
            let (_, content) = self.file_content(&options.file)?;
            let rendered = self.render(&content, options.params.as_ref(), 0)?;
            return Ok(self.apply_i18n(rendered, options.params.as_ref()));
        }
        let (_, content) = self.file_content(&options.file)?;
        if content.is_empty() {
            return Ok(String::new());
        }
        let rendered = self.render(&content, options.params.as_ref(), 0)?;
        Ok(self.apply_i18n(rendered, options.params.as_ref()))
    }

    /// Reads a template file (or resource) through the cache, refreshing it on
    /// mtime changes.
    fn file_content(&self, file: &str) -> Result<(String, String), Error> {
        let (path, _folder, resource) = self.search_file(file)?;
        if let Some(content) = resource {
            return Ok((path, content));
        }
        let modified = std::fs::metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        {
            let cache = self
                .inner
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(item) = cache.get(file)
                && item.path == path
                && item.modified == modified
            {
                return Ok((path, item.content.clone()));
            }
        }
        let content = gfile::get_contents(&path);
        self.inner
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                file.to_owned(),
                CacheItem {
                    path: path.clone(),
                    content: content.clone(),
                    modified,
                },
            );
        Ok((path, content))
    }

    /// Searches `file`, returning `(path, folder, resource content)`.
    fn search_file(&self, file: &str) -> Result<(String, String, Option<String>), Error> {
        let trimmed = file.trim_start_matches(['/', '\\']);
        let paths = self
            .inner
            .paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        // Resource manager first.
        if !gres::is_empty() {
            for base in &paths {
                let candidate = format!("{}/{}", base.trim_end_matches(['/', '\\']), trimmed);
                if let Some(resource) = gres::get(&candidate) {
                    return Ok((
                        resource.name().to_owned(),
                        base.clone(),
                        Some(String::from_utf8_lossy(resource.content()).into_owned()),
                    ));
                }
                for try_folder in RESOURCE_TRY_FOLDERS {
                    let candidate = format!(
                        "{}/{}/{}",
                        base.trim_end_matches(['/', '\\']),
                        try_folder.trim_end_matches(['/', '\\']),
                        file
                    );
                    if let Some(resource) = gres::get(&candidate) {
                        return Ok((
                            resource.name().to_owned(),
                            base.clone(),
                            Some(String::from_utf8_lossy(resource.content()).into_owned()),
                        ));
                    }
                }
            }
            for try_folder in RESOURCE_TRY_FOLDERS {
                let candidate = format!("{}/{}", try_folder.trim_end_matches(['/', '\\']), trimmed);
                if let Some(resource) = gres::get(&candidate) {
                    return Ok((
                        resource.name().to_owned(),
                        try_folder.to_owned(),
                        Some(String::from_utf8_lossy(resource.content()).into_owned()),
                    ));
                }
            }
        }
        // Absolute file.
        let absolute = gfile::real_path(file);
        if !absolute.is_empty() && gfile::is_file(&absolute) {
            return Ok((absolute.clone(), gfile::dir(&absolute), None));
        }
        // Search paths.
        for base in &paths {
            for try_folder in LOCAL_TRY_FOLDERS {
                let candidate = gfile::join(&[base, try_folder, file]);
                if gfile::is_file(&candidate) {
                    return Ok((candidate.clone(), gfile::dir(&candidate), None));
                }
            }
        }
        Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!("template file \"{file}\" not found"),
            Vec::new(),
        ))
    }

    /// Renders `content` with `params`, merging the view data over them.
    fn render(
        &self,
        content: &str,
        params: Option<&Params>,
        depth: usize,
    ) -> Result<String, Error> {
        let (left, right, auto_encode) = {
            let config = self
                .inner
                .config
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                config.delimiters.0.clone(),
                config.delimiters.1.clone(),
                config.auto_encode,
            )
        };
        let (content, definitions) = split_definitions(content)?;
        let tokens = split_template(&content, &left, &right)?;
        let nodes = NodeParser::new(tokens).parse_all()?;
        let mut merged: Params = params.cloned().unwrap_or_default();
        {
            let data = self
                .inner
                .data
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for (key, value) in data.iter() {
                merged.insert(key.clone(), value.clone());
            }
        }
        let root = Value::Map(merged);
        let mut ctx = RenderCtx {
            view: self,
            root,
            dot: Value::Null,
            vars: HashMap::new(),
            auto_encode,
            depth,
            definitions: &definitions,
        };
        ctx.dot = ctx.root.clone();
        let mut output = String::new();
        render_nodes(&nodes, &mut ctx, &mut output)?;
        Ok(output)
    }
}

/// Resource folders tried when looking up templates.
const RESOURCE_TRY_FOLDERS: [&str; 8] = [
    "template/",
    "template",
    "/template",
    "/template/",
    "resource/template/",
    "resource/template",
    "/resource/template",
    "/resource/template/",
];

/// Local folders tried when looking up templates.
const LOCAL_TRY_FOLDERS: [&str; 3] = ["", "template/", "resource/template"];

// ---------------------------------------------------------------------------
// Template lexer
// ---------------------------------------------------------------------------

/// One raw template token.
#[derive(Clone)]
enum RawToken {
    Text(String),
    Action(String),
}

/// Splits template content into text and action tokens, honouring trim
/// markers and comments.
fn split_template(content: &str, left: &str, right: &str) -> Result<Vec<RawToken>, Error> {
    let mut tokens = Vec::new();
    let mut rest = content;
    let mut trim_next = false;
    while let Some(start) = rest.find(left) {
        let mut text = rest[..start].to_owned();
        if trim_next {
            text = text.trim_start().to_owned();
        }
        let after_left = &rest[start + left.len()..];
        let trim_left = after_left.starts_with('-');
        let after_left = if trim_left {
            &after_left[1..]
        } else {
            after_left
        };
        if trim_left {
            text = text.trim_end().to_owned();
        }
        let (action, consumed) = find_action_end(after_left, right)?;
        let trim_right = action.trim_end().ends_with('-');
        let action = if trim_right {
            action.trim_end().trim_end_matches('-').to_owned()
        } else {
            action.to_owned()
        };
        rest = &after_left[consumed..];
        if !text.is_empty() {
            tokens.push(RawToken::Text(text));
        }
        let cleaned = action.trim();
        if !cleaned.is_empty() && !(cleaned.starts_with("/*") && cleaned.ends_with("*/")) {
            tokens.push(RawToken::Action(cleaned.to_owned()));
        }
        trim_next = trim_right;
    }
    let trailing = if trim_next { rest.trim_start() } else { rest };
    if !trailing.is_empty() {
        tokens.push(RawToken::Text(trailing.to_owned()));
    }
    Ok(tokens)
}

/// Finds the closing delimiter outside a quoted string, returning the action
/// text and the byte count consumed including the delimiter.
fn find_action_end<'a>(text: &'a str, right: &str) -> Result<(&'a str, usize), Error> {
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;
    while index < text.len() {
        let character = text[index..].chars().next().unwrap_or('\0');
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            index += character.len_utf8();
            continue;
        }
        if character == '"' {
            in_string = true;
            index += 1;
            continue;
        }
        if text[index..].starts_with(right) {
            return Ok((&text[..index], index + right.len()));
        }
        index += character.len_utf8();
    }
    Err(gerror::new(
        "template: unexpected end of action, missing closing delimiter",
    ))
}

// ---------------------------------------------------------------------------
// Template AST
// ---------------------------------------------------------------------------

/// One template node.
enum Node {
    Text(String),
    Action(Expr),
    If {
        branches: Vec<(Expr, Vec<Node>)>,
        otherwise: Vec<Node>,
    },
    Range {
        key_var: Option<String>,
        value_var: Option<String>,
        expr: Expr,
        body: Vec<Node>,
        otherwise: Vec<Node>,
    },
    With {
        expr: Expr,
        body: Vec<Node>,
        otherwise: Vec<Node>,
    },
    Template {
        name: String,
        pipeline: Option<Expr>,
    },
}

/// One template expression.
enum Expr {
    Literal(Value),
    Field(String),
    Variable(String, String),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Pipeline {
        first: Box<Expr>,
        stages: Vec<(String, Vec<Expr>)>,
    },
}

/// A statement parser over raw tokens.
struct NodeParser {
    tokens: Vec<RawToken>,
    position: usize,
}

impl NodeParser {
    fn new(tokens: Vec<RawToken>) -> NodeParser {
        NodeParser {
            tokens,
            position: 0,
        }
    }

    fn parse_all(mut self) -> Result<Vec<Node>, Error> {
        let (nodes, stop) = self.parse_block(&[])?;
        if let Some(stop) = stop {
            return Err(gerror::new(format!(
                "template: unexpected \"{stop}\" at top level"
            )));
        }
        Ok(nodes)
    }

    /// Parses nodes until one of `stops` or the end, returning the stopping
    /// action text (already consumed).
    fn parse_block(&mut self, stops: &[&str]) -> Result<(Vec<Node>, Option<String>), Error> {
        let mut nodes = Vec::new();
        while self.position < self.tokens.len() {
            match self.tokens[self.position].clone() {
                RawToken::Text(text) => {
                    nodes.push(Node::Text(text));
                    self.position += 1;
                }
                RawToken::Action(action) => {
                    if stop_matches(&action, stops) {
                        self.position += 1;
                        return Ok((nodes, Some(action)));
                    }
                    self.position += 1;
                    if action == "end" || action == "else" || action.starts_with("else if ") {
                        return Err(gerror::new(format!("template: unexpected \"{action}\"")));
                    }
                    nodes.push(self.parse_statement(&action)?);
                }
            }
        }
        Ok((nodes, None))
    }

    /// Parses one action statement, consuming its sub-blocks when needed.
    fn parse_statement(&mut self, action: &str) -> Result<Node, Error> {
        if let Some(rest) = action.strip_prefix("if ") {
            return self.parse_if(rest);
        }
        if let Some(rest) = action.strip_prefix("range ") {
            return self.parse_range(rest);
        }
        if let Some(rest) = action.strip_prefix("with ") {
            return self.parse_with(rest);
        }
        if let Some(rest) = action.strip_prefix("template ") {
            let (name, pipeline_text) = parse_template_call(rest)?;
            let pipeline = if pipeline_text.is_empty() {
                None
            } else {
                Some(parse_expr_text(&pipeline_text)?)
            };
            return Ok(Node::Template { name, pipeline });
        }
        if action.starts_with("define ") || action.starts_with("block ") {
            return Err(gerror::new(format!(
                "template: action \"{action}\" must be resolved before parsing"
            )));
        }
        Ok(Node::Action(parse_expr_text(action)?))
    }

    /// Parses an `if` chain.
    fn parse_if(&mut self, condition: &str) -> Result<Node, Error> {
        let condition = parse_expr_text(condition)?;
        let (body, stop) = self.parse_block(&["else", "end"])?;
        match stop.as_deref() {
            Some("end") => Ok(Node::If {
                branches: vec![(condition, body)],
                otherwise: Vec::new(),
            }),
            Some("else") => {
                let (otherwise, stop) = self.parse_block(&["end"])?;
                if stop.as_deref() != Some("end") {
                    return Err(gerror::new("template: missing \"end\" for \"if\""));
                }
                Ok(Node::If {
                    branches: vec![(condition, body)],
                    otherwise,
                })
            }
            Some(stop) if stop.starts_with("else if ") => {
                let inner = self.parse_if(&stop["else if ".len()..])?;
                Ok(Node::If {
                    branches: vec![(condition, body)],
                    otherwise: vec![inner],
                })
            }
            Some(stop) => Err(gerror::new(format!(
                "template: unexpected \"{stop}\" in \"if\""
            ))),
            None => Err(gerror::new("template: missing \"end\" for \"if\"")),
        }
    }

    fn parse_range(&mut self, header: &str) -> Result<Node, Error> {
        let (key_var, value_var, expr_text) = parse_range_header(header)?;
        let expr = parse_expr_text(&expr_text)?;
        let (body, stop) = self.parse_block(&["else", "end"])?;
        let otherwise = match stop.as_deref() {
            Some("end") => Vec::new(),
            None => return Err(gerror::new("template: missing \"end\"")),
            Some("else") => {
                let (otherwise, stop) = self.parse_block(&["end"])?;
                if stop.as_deref() != Some("end") {
                    return Err(gerror::new("template: missing \"end\" for \"range\""));
                }
                otherwise
            }
            Some(stop) => {
                return Err(gerror::new(format!(
                    "template: unexpected \"{stop}\" in \"range\""
                )));
            }
        };
        Ok(Node::Range {
            key_var,
            value_var,
            expr,
            body,
            otherwise,
        })
    }

    fn parse_with(&mut self, header: &str) -> Result<Node, Error> {
        let expr = parse_expr_text(header)?;
        let (body, stop) = self.parse_block(&["else", "end"])?;
        let otherwise = match stop.as_deref() {
            Some("end") => Vec::new(),
            None => return Err(gerror::new("template: missing \"end\"")),
            Some("else") => {
                let (otherwise, stop) = self.parse_block(&["end"])?;
                if stop.as_deref() != Some("end") {
                    return Err(gerror::new("template: missing \"end\" for \"with\""));
                }
                otherwise
            }
            Some(stop) => {
                return Err(gerror::new(format!(
                    "template: unexpected \"{stop}\" in \"with\""
                )));
            }
        };
        Ok(Node::With {
            expr,
            body,
            otherwise,
        })
    }
}

/// Reports whether `action` is one of the block terminators.
fn stop_matches(action: &str, stops: &[&str]) -> bool {
    stops
        .iter()
        .any(|stop| action == *stop || (*stop == "else" && action.starts_with("else if ")))
}

/// Parses the `range` header, returning the variables and the collection
/// Parses a `template "name" [pipeline]` action.
fn parse_template_call(rest: &str) -> Result<(String, String), Error> {
    let rest = rest.trim();
    let (name, after) = parse_quoted_name(rest)?;
    Ok((name, after.trim().to_owned()))
}

/// Parses a leading `"name"` from `text`, returning it and the remainder.
fn parse_quoted_name(text: &str) -> Result<(String, String), Error> {
    let text = text.trim_start();
    let Some(body) = text.strip_prefix('"') else {
        return Err(gerror::new("template: expected a quoted template name"));
    };
    let Some(end) = body.find('"') else {
        return Err(gerror::new("template: unterminated template name string"));
    };
    Ok((body[..end].to_owned(), body[end + 1..].to_owned()))
}

/// Splits `{{define}}` blocks and expands `{{block}}` into `{{template}}`.
///
/// Returns the remaining content and the definition map. Definitions must be
/// at the top level, and the matching `{{end}}` is found with the same depth
/// counting the parser uses for `if`/`range`/`with`.
fn split_definitions(content: &str) -> Result<(String, HashMap<String, String>), Error> {
    let mut definitions = HashMap::new();
    let mut output = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let Some(relative_close) = rest[start + 2..].find("}}") else {
            output.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let close = start + 2 + relative_close;
        let action = rest[start + 2..close].trim();
        let body_start = close + 2;
        if let Some(after_keyword) = action.strip_prefix("define ") {
            let (name, _) = parse_quoted_name(after_keyword)?;
            let (body, after) = find_matching_end(rest, body_start)?;
            definitions.insert(name, body.to_owned());
            rest = &rest[after..];
            continue;
        }
        if let Some(after_keyword) = action.strip_prefix("block ") {
            let (name, pipeline) = parse_quoted_name(after_keyword)?;
            let (body, after) = find_matching_end(rest, body_start)?;
            definitions.insert(name.clone(), body.to_owned());
            let pipeline = pipeline.trim();
            if pipeline.is_empty() {
                output.push_str(&format!("{{{{template \"{name}\"}}}}"));
            } else {
                output.push_str(&format!("{{{{template \"{name}\" {pipeline}}}}}"));
            }
            rest = &rest[after..];
            continue;
        }
        output.push_str(&rest[..body_start]);
        rest = &rest[body_start..];
    }
    output.push_str(rest);
    Ok((output, definitions))
}

/// Finds the `{{end}}` matching the block that starts at `body_start`.
fn find_matching_end(text: &str, body_start: usize) -> Result<(&str, usize), Error> {
    let mut depth = 1usize;
    let mut cursor = body_start;
    loop {
        let Some(relative_start) = text[cursor..].find("{{") else {
            return Err(gerror::new("template: missing \"end\" for a definition"));
        };
        let start = cursor + relative_start;
        let Some(relative_close) = text[start + 2..].find("}}") else {
            return Err(gerror::new(
                "template: unexpected end of action, missing closing delimiter",
            ));
        };
        let close = start + 2 + relative_close;
        let action = text[start + 2..close].trim();
        if is_block_opener(action) {
            depth += 1;
        } else if action == "end" {
            depth -= 1;
            if depth == 0 {
                return Ok((&text[body_start..start], close + 2));
            }
        }
        cursor = close + 2;
    }
}

/// Reports whether `action` opens a block that needs a matching `end`.
fn is_block_opener(action: &str) -> bool {
    ["if ", "range ", "with ", "block ", "define "]
        .iter()
        .any(|prefix| action.starts_with(prefix))
}

/// Parses the `range` header, returning the variables and the collection
/// expression text.
fn parse_range_header(header: &str) -> Result<(Option<String>, Option<String>, String), Error> {
    if let Some((vars, expr)) = header.split_once(":=") {
        let vars = vars.trim();
        if vars.contains(',') {
            let mut parts = vars.split(',');
            let key = parts.next().unwrap_or_default().trim();
            let value = parts.next().unwrap_or_default().trim();
            if !key.starts_with('$') || !value.starts_with('$') {
                return Err(gerror::new("template: invalid range variable list"));
            }
            return Ok((
                Some(key.trim_start_matches('$').to_owned()),
                Some(value.trim_start_matches('$').to_owned()),
                expr.trim().to_owned(),
            ));
        }
        if !vars.starts_with('$') {
            return Err(gerror::new("template: invalid range variable"));
        }
        return Ok((
            None,
            Some(vars.trim_start_matches('$').to_owned()),
            expr.trim().to_owned(),
        ));
    }
    Ok((None, None, header.trim().to_owned()))
}

// ---------------------------------------------------------------------------
// Expression lexer and parser
// ---------------------------------------------------------------------------

/// Splits an action into expression tokens.
fn tokenize_expr(text: &str) -> Result<Vec<String>, Error> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                let mut literal = String::from("\"");
                let mut escaped = false;
                let mut closed = false;
                for inner in characters.by_ref() {
                    literal.push(inner);
                    if escaped {
                        escaped = false;
                    } else if inner == '\\' {
                        escaped = true;
                    } else if inner == '"' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(gerror::new("template: unterminated string literal"));
                }
                tokens.push(literal);
            }
            '|' | '(' | ')' | ',' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(character.to_string());
            }
            ':' => {
                if characters.peek() == Some(&'=') {
                    characters.next();
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                    tokens.push(":=".to_owned());
                } else {
                    current.push(character);
                }
            }
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Parses an action expression from text.
fn parse_expr_text(text: &str) -> Result<Expr, Error> {
    let tokens = tokenize_expr(text)?;
    parse_expr(&tokens)
}

/// Parses an expression token list.
fn parse_expr(tokens: &[String]) -> Result<Expr, Error> {
    if tokens.is_empty() {
        return Err(gerror::new("template: empty action"));
    }
    let stages = split_pipes(tokens);
    let first = parse_stage(stages[0])?;
    if stages.len() == 1 {
        return Ok(first);
    }
    let mut pipeline = Vec::new();
    for stage in &stages[1..] {
        let (name, args) = parse_call(stage)?;
        pipeline.push((name, args));
    }
    Ok(Expr::Pipeline {
        first: Box::new(first),
        stages: pipeline,
    })
}

/// Splits tokens on `|`.
fn split_pipes(tokens: &[String]) -> Vec<&[String]> {
    let mut stages = Vec::new();
    let mut start = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if token == "|" {
            stages.push(&tokens[start..index]);
            start = index + 1;
        }
    }
    stages.push(&tokens[start..]);
    stages
}

/// Parses one pipeline stage.
fn parse_stage(tokens: &[String]) -> Result<Expr, Error> {
    if tokens.is_empty() {
        return Err(gerror::new("template: empty expression"));
    }
    if tokens.len() == 1 {
        return parse_term(&tokens[0]);
    }
    if tokens[0] == "(" {
        if tokens.last().map(String::as_str) != Some(")") {
            return Err(gerror::new("template: unbalanced parenthesis"));
        }
        return parse_expr(&tokens[1..tokens.len() - 1]);
    }
    let (name, args) = parse_call(tokens)?;
    Ok(Expr::Call { name, args })
}

/// Parses `name arg...` into a call.
fn parse_call(tokens: &[String]) -> Result<(String, Vec<Expr>), Error> {
    if tokens.is_empty() {
        return Err(gerror::new("template: empty call"));
    }
    let name = tokens[0].clone();
    if !is_identifier(&name) {
        return Err(gerror::new(format!(
            "template: unexpected function name \"{name}\""
        )));
    }
    let args = parse_terms(&tokens[1..])?;
    Ok((name, args))
}

/// Parses a term list (arguments), handling parenthesized groups.
fn parse_terms(tokens: &[String]) -> Result<Vec<Expr>, Error> {
    let mut terms = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        if tokens[index] == "(" {
            let mut depth = 1usize;
            let mut end = index + 1;
            while end < tokens.len() && depth > 0 {
                match tokens[end].as_str() {
                    "(" => depth += 1,
                    ")" => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            if depth != 0 {
                return Err(gerror::new("template: unbalanced parenthesis"));
            }
            terms.push(parse_expr(&tokens[index + 1..end - 1])?);
            index = end;
        } else {
            terms.push(parse_term(&tokens[index])?);
            index += 1;
        }
    }
    Ok(terms)
}

/// Parses one term.
fn parse_term(token: &str) -> Result<Expr, Error> {
    if token.starts_with('"') && token.ends_with('"') && token.len() >= 2 {
        return Ok(Expr::Literal(Value::String(unescape_string(
            &token[1..token.len() - 1],
        ))));
    }
    if token == "." {
        return Ok(Expr::Field(String::new()));
    }
    if let Some(path) = token.strip_prefix('.') {
        return Ok(Expr::Field(path.to_owned()));
    }
    if let Some(variable) = token.strip_prefix('$') {
        let (name, path) = match variable.split_once('.') {
            Some((name, path)) => (name, path),
            None => (variable, ""),
        };
        return Ok(Expr::Variable(name.to_owned(), path.to_owned()));
    }
    if token == "true" {
        return Ok(Expr::Literal(Value::Bool(true)));
    }
    if token == "false" {
        return Ok(Expr::Literal(Value::Bool(false)));
    }
    if token == "nil" || token == "null" {
        return Ok(Expr::Literal(Value::Null));
    }
    if let Ok(value) = token.parse::<i64>() {
        return Ok(Expr::Literal(Value::Int(value)));
    }
    if let Ok(value) = token.parse::<f64>() {
        return Ok(Expr::Literal(Value::Float(value)));
    }
    if is_identifier(token) {
        // A bare identifier is a zero-argument function call in Go templates.
        return Ok(Expr::Call {
            name: token.to_owned(),
            args: Vec::new(),
        });
    }
    Err(gerror::new(format!(
        "template: unsupported expression token \"{token}\""
    )))
}

/// Reports whether `token` is a Go-style identifier.
fn is_identifier(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !token.starts_with(|character: char| character.is_ascii_digit())
}

/// Unescapes a quoted string body.
fn unescape_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            output.push(match character {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    output
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

/// A rendered value; `safe` values skip automatic HTML encoding.
struct Out {
    value: Value,
    safe: bool,
}

/// The render context.
struct RenderCtx<'a> {
    view: &'a View,
    root: Value,
    dot: Value,
    vars: HashMap<String, Value>,
    auto_encode: bool,
    depth: usize,
    definitions: &'a HashMap<String, String>,
}

/// Renders a node list.
fn render_nodes(nodes: &[Node], ctx: &mut RenderCtx, output: &mut String) -> Result<(), Error> {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_str(text),
            Node::Action(expr) => {
                let rendered = eval_expr(expr, ctx)?;
                let mut text = format_value(&rendered.value);
                if ctx.auto_encode && !rendered.safe {
                    text = ghtml::entities(&text);
                }
                output.push_str(&text);
            }
            Node::If {
                branches,
                otherwise,
            } => {
                let mut matched = false;
                for (condition, body) in branches {
                    if truthy(&eval_expr(condition, ctx)?.value) {
                        render_nodes(body, ctx, output)?;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    render_nodes(otherwise, ctx, output)?;
                }
            }
            Node::Range {
                key_var,
                value_var,
                expr,
                body,
                otherwise,
            } => {
                let collection = eval_expr(expr, ctx)?.value;
                let entries = range_entries(&collection);
                if entries.is_empty() {
                    render_nodes(otherwise, ctx, output)?;
                    continue;
                }
                let saved_dot = ctx.dot.clone();
                let saved_vars = ctx.vars.clone();
                for (key, value) in entries {
                    ctx.dot = value.clone();
                    if let Some(key_var) = key_var {
                        ctx.vars.insert(key_var.clone(), key);
                    }
                    if let Some(value_var) = value_var {
                        ctx.vars.insert(value_var.clone(), value);
                    }
                    render_nodes(body, ctx, output)?;
                }
                ctx.dot = saved_dot;
                ctx.vars = saved_vars;
            }
            Node::Template { name, pipeline } => {
                let Some(body) = ctx.definitions.get(name).cloned() else {
                    return Err(gerror::new(format!(
                        "template: no template \"{name}\" associated with this engine"
                    )));
                };
                let data = match pipeline {
                    Some(expr) => eval_expr(expr, ctx)?.value,
                    None => ctx.dot.clone(),
                };
                if ctx.depth > 100 {
                    return Err(gerror::new("template: too many nested template calls"));
                }
                let (left, right, auto_encode) = {
                    let config = ctx
                        .view
                        .inner
                        .config
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    (
                        config.delimiters.0.clone(),
                        config.delimiters.1.clone(),
                        config.auto_encode,
                    )
                };
                let tokens = split_template(&body, &left, &right)?;
                let nodes = NodeParser::new(tokens).parse_all()?;
                let mut child = RenderCtx {
                    view: ctx.view,
                    root: data.clone(),
                    dot: data,
                    vars: HashMap::new(),
                    auto_encode,
                    depth: ctx.depth + 1,
                    definitions: ctx.definitions,
                };
                render_nodes(&nodes, &mut child, output)?;
            }
            Node::With {
                expr,
                body,
                otherwise,
            } => {
                let value = eval_expr(expr, ctx)?.value;
                if truthy(&value) {
                    let saved_dot = ctx.dot.clone();
                    ctx.dot = value;
                    render_nodes(body, ctx, output)?;
                    ctx.dot = saved_dot;
                } else {
                    render_nodes(otherwise, ctx, output)?;
                }
            }
        }
    }
    Ok(())
}

/// Flattens a collection into `(key, value)` range entries.
fn range_entries(value: &Value) -> Vec<(Value, Value)> {
    match value {
        Value::List(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (Value::Int(index as i64), item.clone()))
            .collect(),
        Value::Map(map) => map
            .iter()
            .map(|(key, value)| (Value::String(key.clone()), value.clone()))
            .collect(),
        Value::Bytes(bytes) => bytes
            .iter()
            .enumerate()
            .map(|(index, byte)| (Value::Int(index as i64), Value::Int(*byte as i64)))
            .collect(),
        Value::Int(count) if *count > 0 => (0..*count)
            .map(|index| (Value::Int(index), Value::Int(index)))
            .collect(),
        _ => Vec::new(),
    }
}

/// Evaluates one expression.
fn eval_expr(expr: &Expr, ctx: &mut RenderCtx) -> Result<Out, Error> {
    match expr {
        Expr::Literal(value) => Ok(Out {
            value: value.clone(),
            safe: false,
        }),
        Expr::Field(path) => Ok(Out {
            value: lookup_field(&ctx.dot, path),
            safe: false,
        }),
        Expr::Variable(name, path) => {
            let base = if name.is_empty() {
                ctx.root.clone()
            } else if let Some(value) = ctx.vars.get(name) {
                value.clone()
            } else {
                lookup_field(&ctx.root, name)
            };
            Ok(Out {
                value: lookup_field(&base, path),
                safe: false,
            })
        }
        Expr::Call { name, args } => call_function(name, args, ctx),
        Expr::Pipeline { first, stages } => {
            let mut current = eval_expr(first, ctx)?.value;
            for (name, args) in stages {
                let mut evaluated = Vec::with_capacity(args.len() + 1);
                for argument in args {
                    evaluated.push(eval_expr(argument, ctx)?.value);
                }
                evaluated.push(current);
                current = call_function(name, &literal_args(&evaluated), ctx)?.value;
            }
            Ok(Out {
                value: current,
                safe: true,
            })
        }
    }
}

/// Wraps evaluated values as literal expressions for direct calls.
fn literal_args(values: &[Value]) -> Vec<Expr> {
    values
        .iter()
        .map(|value| Expr::Literal(value.clone()))
        .collect()
}

/// Calls one template function.
fn call_function(name: &str, args: &[Expr], ctx: &mut RenderCtx) -> Result<Out, Error> {
    if name == "include" {
        return call_include(args, ctx);
    }
    let mut values = Vec::with_capacity(args.len());
    for argument in args {
        values.push(eval_expr(argument, ctx)?.value);
    }
    if let Some(function) = ctx
        .view
        .inner
        .funcs
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(name)
        .cloned()
    {
        return Ok(Out {
            value: function(&values)?,
            safe: true,
        });
    }
    call_builtin(name, &values, ctx)
}

/// Implements the `include` function.
fn call_include(args: &[Expr], ctx: &mut RenderCtx) -> Result<Out, Error> {
    if args.is_empty() {
        return Err(gerror::new("template: include requires a file name"));
    }
    let file = gconv::string(&eval_expr(&args[0], ctx)?.value);
    if file.is_empty() {
        return Ok(Out {
            value: Value::String(String::new()),
            safe: true,
        });
    }
    if ctx.depth >= 32 {
        return Err(gerror::new("template: include depth exceeded"));
    }
    let mut params = Params::new();
    if let Some(data) = args.get(1)
        && let Value::Map(map) = eval_expr(data, ctx)?.value
    {
        params = map;
    }
    let content = match ctx.view.file_content(&file) {
        Ok((_, content)) => content,
        Err(error) => {
            return Ok(Out {
                value: Value::String(error.to_string()),
                safe: true,
            });
        }
    };
    match ctx.view.render(&content, Some(&params), ctx.depth + 1) {
        Ok(rendered) => Ok(Out {
            value: Value::String(rendered),
            safe: true,
        }),
        Err(error) => Ok(Out {
            value: Value::String(error.to_string()),
            safe: true,
        }),
    }
}

/// Calls one built-in function.
fn call_builtin(name: &str, args: &[Value], _ctx: &mut RenderCtx) -> Result<Out, Error> {
    let string =
        |index: usize| -> String { args.get(index).map(gconv::string).unwrap_or_default() };
    let number =
        |index: usize| -> i64 { args.get(index).map(gconv::int64_value).unwrap_or_default() };
    let float = |index: usize| -> f64 {
        args.get(index)
            .map(gconv::float64_value)
            .unwrap_or_default()
    };
    let out = |value: Value| Ok(Out { value, safe: true });
    match name {
        "eq" => {
            let first = string(0);
            let matched = args
                .iter()
                .skip(1)
                .any(|other| gconv::string(other) == first);
            out(Value::Bool(matched))
        }
        "ne" => out(Value::Bool(string(0) != string(1))),
        "lt" | "le" | "gt" | "ge" => {
            let (left, right) = (string(0), string(1));
            let result = if gstr::is_numeric(&left) && gstr::is_numeric(&right) {
                let (left, right) = (gconv::int64_value(&args[0]), gconv::int64_value(&args[1]));
                match name {
                    "lt" => left < right,
                    "le" => left <= right,
                    "gt" => left > right,
                    _ => left >= right,
                }
            } else {
                let ordering = gstr::compare(&left, &right);
                match name {
                    "lt" => ordering < 0,
                    "le" => ordering <= 0,
                    "gt" => ordering > 0,
                    _ => ordering >= 0,
                }
            };
            out(Value::Bool(result))
        }
        "text" => out(Value::String(ghtml::strip_tags(&string(0)))),
        "html" | "htmlencode" | "encode" => out(Value::String(ghtml::entities(&string(0)))),
        "htmldecode" | "decode" => out(Value::String(ghtml::entities_decode(&string(0)))),
        "url" | "urlencode" => out(Value::String(gurl::encode(&string(0)))),
        "urldecode" => match gurl::decode(&string(0)) {
            Ok(decoded) => out(Value::String(decoded)),
            Err(error) => out(Value::String(error.to_string())),
        },
        "date" => {
            let format = string(0);
            let timestamp = if args.len() > 1 {
                number(1)
            } else {
                gtime::timestamp()
            };
            out(Value::String(
                gtime::Time::new_from_timestamp(timestamp).format(&format),
            ))
        }
        "substr" => {
            let start = number(0) as i32;
            let length = if args.len() > 2 { number(1) as i32 } else { -1 };
            let value = if args.len() > 2 { string(2) } else { string(1) };
            out(Value::String(sub_str_rune(&value, start, length)))
        }
        "strlimit" => {
            let length = number(0) as usize;
            let suffix = if args.len() > 2 {
                string(1)
            } else {
                String::new()
            };
            let value = if args.len() > 2 { string(2) } else { string(1) };
            out(Value::String(str_limit_rune(&value, length, &suffix)))
        }
        "concat" => {
            let mut result = String::new();
            for value in args {
                result.push_str(&gconv::string(value));
            }
            out(Value::String(result))
        }
        "replace" => {
            let value = string(2);
            out(Value::String(gfile_replace(&value, &string(0), &string(1))))
        }
        "compare" => out(Value::Int(gstr::compare(&string(0), &string(1)) as i64)),
        "hidestr" => out(Value::String(hide_str(
            &string(2),
            number(0) as i32,
            &string(1),
        ))),
        "highlight" => {
            let replaced = string(2).replace(
                &string(0),
                &format!("<span style=\"color:{};\">{}</span>", string(1), string(0)),
            );
            out(Value::String(replaced))
        }
        "toupper" => out(Value::String(gstr::to_upper(&string(0)))),
        "tolower" => out(Value::String(gstr::to_lower(&string(0)))),
        "nl2br" => out(Value::String(nl2br(&string(0)))),
        "dump" => out(Value::String(dump_values(args))),
        "map" => {
            let value = match args.first() {
                Some(Value::Map(map)) => Value::Map(map.clone()),
                Some(other) => Value::Map(gconv::map(other)),
                None => Value::Map(IndexMap::new()),
            };
            out(value)
        }
        "maps" => {
            let value = match args.first() {
                Some(Value::List(items)) => Value::List(
                    items
                        .iter()
                        .filter(|item| matches!(item, Value::Map(_)))
                        .cloned()
                        .collect(),
                ),
                _ => Value::List(Vec::new()),
            };
            out(value)
        }
        "json" => match gf_json::marshal_value(args.first().unwrap_or(&Value::Null)) {
            Ok(text) => out(Value::String(text)),
            Err(error) => Err(error),
        },
        "xml" => match gjson::Json::new(args.first().cloned().unwrap_or(Value::Null)).to_xml() {
            Ok(text) => out(Value::String(text)),
            Err(error) => Err(error),
        },
        "ini" => match gjson::Json::new(args.first().cloned().unwrap_or(Value::Null)).to_ini() {
            Ok(text) => out(Value::String(text)),
            Err(error) => Err(error),
        },
        "yaml" | "yamli" => {
            match gjson::Json::new(args.first().cloned().unwrap_or(Value::Null)).to_yaml() {
                Ok(text) => out(Value::String(text)),
                Err(error) => Err(error),
            }
        }
        "toml" => match gjson::Json::new(args.first().cloned().unwrap_or(Value::Null)).to_toml() {
            Ok(text) => out(Value::String(text)),
            Err(error) => Err(error),
        },
        "plus" | "minus" | "times" | "divide" => {
            let mut result = float(0);
            for value in args.iter().skip(1) {
                let operand = gconv::float64_value(value);
                match name {
                    "plus" => result += operand,
                    "minus" => result -= operand,
                    "times" => result *= operand,
                    _ => {
                        if operand == 0.0 {
                            return out(Value::String("0".to_owned()));
                        }
                        result /= operand;
                    }
                }
            }
            out(Value::String(format_float(result)))
        }
        _ => Err(gerror::new(format!(
            "template: function \"{name}\" is not defined"
        ))),
    }
}

/// Looks up a dotted field path in a value.
fn lookup_field(value: &Value, path: &str) -> Value {
    if path.is_empty() {
        return value.clone();
    }
    let mut current = value.clone();
    for part in path.split('.') {
        current = match &current {
            Value::Map(map) => map.get(part).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        };
        if matches!(current, Value::Null) {
            break;
        }
    }
    current
}

/// Reports whether a value is truthy in an `if`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Int(value) => *value != 0,
        Value::UInt(value) => *value != 0,
        Value::Float(value) => *value != 0.0,
        Value::String(value) => !value.is_empty(),
        Value::Bytes(value) => !value.is_empty(),
        Value::List(value) => !value.is_empty(),
        Value::Map(value) => !value.is_empty(),
    }
}

/// Formats a value for output.
fn format_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => gconv::string(other),
    }
}

/// Formats a float like Go's `%v` (no trailing `.0`).
fn format_float(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `gstr.SubStrRune`: substring by rune index with optional negative length.
fn sub_str_rune(value: &str, start: i32, length: i32) -> String {
    let runes: Vec<char> = value.chars().collect();
    let total = runes.len() as i32;
    let mut start = start;
    if start < 0 {
        start = if -start > total { 0 } else { total + start };
    } else if start > total {
        return String::new();
    }
    let mut real_length = if length < 0 {
        if -length > total - start {
            0
        } else {
            total - start + length
        }
    } else if length > total - start {
        total - start
    } else {
        length
    };
    if real_length < 0 {
        real_length = 0;
    }
    runes[start as usize..(start + real_length) as usize]
        .iter()
        .collect()
}

/// `gstr.StrLimitRune`: truncate to `length` runes and append `suffix`.
fn str_limit_rune(value: &str, length: usize, suffix: &str) -> String {
    let runes: Vec<char> = value.chars().collect();
    if runes.len() <= length {
        return value.to_owned();
    }
    let mut result: String = runes[..length].iter().collect();
    result.push_str(suffix);
    result
}

/// `gstr.HideStr`: hide the middle `percent` of the runes with `hide`.
fn hide_str(value: &str, percent: i32, hide: &str) -> String {
    let (body, suffix) = match value.find('@') {
        Some(index) => (&value[..index], &value[index..]),
        None => (value, ""),
    };
    if body.is_empty() || percent <= 0 {
        return format!("{body}{suffix}");
    }
    let runes: Vec<char> = body.chars().collect();
    let length = runes.len();
    if percent >= 100 {
        return format!("{}{suffix}", hide.repeat(length));
    }
    let hide_length = (length * percent as usize) / 100;
    if hide_length == 0 {
        return format!("{body}{suffix}");
    }
    let mid = length / 2;
    let mut start = mid.saturating_sub(hide_length / 2);
    let mut end = start + hide_length;
    if end > length {
        end = length;
        start = length.saturating_sub(hide_length);
    }
    format!(
        "{}{}{}{suffix}",
        runes[..start].iter().collect::<String>(),
        hide.repeat(end - start),
        runes[end..].iter().collect::<String>()
    )
}

/// `gstr.Nl2Br`.
fn nl2br(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let runes: Vec<char> = value.chars().collect();
    let mut index = 0usize;
    while index < runes.len() {
        let character = runes[index];
        if character == '\r' || character == '\n' {
            output.push_str("<br>");
            if index + 1 < runes.len()
                && ((character == '\r' && runes[index + 1] == '\n')
                    || (character == '\n' && runes[index + 1] == '\r'))
            {
                index += 1;
            }
        } else {
            output.push(character);
        }
        index += 1;
    }
    output
}

/// Replaces every occurrence of `search` with `replacement`.
fn gfile_replace(value: &str, search: &str, replacement: &str) -> String {
    if search.is_empty() {
        return value.to_owned();
    }
    value.replace(search, replacement)
}

/// Renders a debug dump for the `dump` function.
fn dump_values(args: &[Value]) -> String {
    let mut output = String::from("\n<!--\n");
    for value in args {
        output.push_str(&format!("{value:?}\n"));
    }
    output.push_str("-->\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, Value)]) -> Params {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    fn sandbox(name: &str) -> String {
        let path = gfile::temp(&["gf-rust-gview-tests", name]);
        let _ = gfile::remove_all(&path);
        gfile::mkdir(&path).expect("sandbox");
        path
    }

    // Translated from Test_Basic.
    #[test]
    fn basic_interpolation_and_functions() {
        let view = View::new();
        view.assign("name", Value::String("gf".to_owned()));
        view.bind_func(
            "GetName",
            Arc::new(|_args: &[Value]| Ok(Value::String("gf".to_owned()))),
        );
        let content = "hello {{.name}},version:{{.version}};hello {{GetName}};{{.other}}";
        let result = view
            .parse_content(
                content,
                Some(&params(&[
                    ("version", Value::String("1.7.0".to_owned())),
                    ("other", Value::String("that's all".to_owned())),
                ])),
            )
            .expect("parse");
        assert_eq!(result, "hello gf,version:1.7.0;hello gf;that's all");
    }

    // Translated from Test_Func comparisons.
    #[test]
    fn comparison_functions() {
        let view = View::new();
        for (template, expected) in [
            (
                "{{eq 1 1}};{{eq 1 2}};{{eq \"A\" \"B\"}}",
                "true;false;false",
            ),
            ("{{eq 1 2 1 3 4 5}}", "true"),
            (
                "{{ne 1 2}};{{ne 1 1}};{{ne \"A\" \"B\"}}",
                "true;false;true",
            ),
            (
                "{{lt 1 2}};{{lt 1 1}};{{lt 1 0}};{{lt \"A\" \"B\"}}",
                "true;false;false;true",
            ),
            (
                "{{le 1 2}};{{le 1 1}};{{le 1 0}};{{le \"A\" \"B\"}}",
                "true;true;false;true",
            ),
            (
                "{{gt 1 2}};{{gt 1 1}};{{gt 1 0}};{{gt \"A\" \"B\"}}",
                "false;false;true;false",
            ),
            (
                "{{ge 1 2}};{{ge 1 1}};{{ge 1 0}};{{ge \"A\" \"B\"}}",
                "false;true;true;false",
            ),
        ] {
            assert_eq!(
                view.parse_content(template, None).expect(template),
                expected
            );
        }
    }

    // Translated from the codec parts of Test_Func.
    #[test]
    fn codec_functions_and_pipelines() {
        let view = View::new();
        for (template, expected) in [
            ("{{\"<div>text</div>\"|text}}", "text"),
            (
                "{{\"<div>text</div>\"|html}}",
                "&lt;div&gt;text&lt;/div&gt;",
            ),
            (
                "{{\"<div>text</div>\"|htmlencode}}",
                "&lt;div&gt;text&lt;/div&gt;",
            ),
            (
                "{{\"&lt;div&gt;text&lt;/div&gt;\"|htmldecode}}",
                "<div>text</div>",
            ),
            (
                "{{\"https://goframe.org\"|url}}",
                "https%3A%2F%2Fgoframe.org",
            ),
            (
                "{{\"https%3A%2F%2Fgoframe.org\"|urldecode}}",
                "https://goframe.org",
            ),
            ("{{\"gf\" | toupper}};{{\"GF\" | tolower}}", "GF;gf"),
            ("{{concat \"I\" \"Love\" \"GoFrame\"}}", "ILoveGoFrame"),
            (
                "{{compare \"A\" \"B\"}};{{compare 2 1}};{{compare 1 1}}",
                "-1;1;0",
            ),
        ] {
            assert_eq!(
                view.parse_content(template, None).expect(template),
                expected
            );
        }
        assert!(
            view.parse_content("{{\"https%3NA%2F%2Fgoframe.org\"|urldecode}}", None)
                .expect("decode")
                .contains("invalid URL escape")
        );
        assert_eq!(
            view.parse_content("{{\"Go\\nFrame\" | nl2br}}", None)
                .expect("nl2br"),
            "Go<br>Frame"
        );
    }

    // Translated from Test_FuncNl2Br with a parameter.
    #[test]
    fn nl2br_over_parameter() {
        let view = View::new();
        let content = "Go\nFrame\n中文";
        let result = view
            .parse_content(
                "{{.content | nl2br}}",
                Some(&params(&[("content", Value::String(content.to_owned()))])),
            )
            .expect("parse");
        assert_eq!(result, content.replace('\n', "<br>"));
    }

    // Translated from the date/substr/strlimit/compare parts of Test_Func.
    #[test]
    fn date_and_string_functions() {
        let view = View::new();
        assert_eq!(
            view.parse_content("{{1540822968 | date \"Y-m-d\"}}", None)
                .expect("date"),
            "2018-10-29"
        );
        assert!(
            view.parse_content("{{date \"Y-m-d\"}}", None)
                .expect("date now")
                .len()
                >= 8
        );
        assert_eq!(
            view.parse_content(
                "{{\"我是中国人\" | substr 2 -1}};{{\"我是中国人\" | substr 2 2}}",
                None
            )
            .expect("substr"),
            "中国;中国"
        );
        assert_eq!(
            view.parse_content("{{\"我是中国人\" | strlimit 2 \"...\"}}", None)
                .expect("strlimit"),
            "我是..."
        );
        assert_eq!(
            view.parse_content("{{\"热爱GF热爱生活\" | highlight \"GF\" \"red\"}}", None)
                .expect("highlight"),
            "热爱<span style=\"color:red;\">GF</span>热爱生活"
        );
        assert_eq!(
            view.parse_content("{{\"gf\" | replace \"gf\" \"GF\"}}", None)
                .expect("replace"),
            "GF"
        );
    }

    // Translated from Test_FuncInclude.
    #[test]
    fn include_resolves_files_and_reports_missing_ones() {
        let base = sandbox("include");
        gfile::put_contents(&gfile::join(&[&base, "header.html"]), "<h1>HEADER</h1>")
            .expect("header");
        gfile::put_contents(&gfile::join(&[&base, "main.html"]), "<h1>hello gf</h1>")
            .expect("main");
        gfile::put_contents(&gfile::join(&[&base, "footer.html"]), "<h1>FOOTER</h1>")
            .expect("footer");
        gfile::put_contents(
            &gfile::join(&[&base, "layout.html"]),
            "{{include \"header.html\" .}}\n{{include \"main.html\" .}}\n{{include \"footer.html\" .}}\n{{include \"footer_not_exist.html\" .}}\n{{include \"\" .}}",
        )
        .expect("layout");

        let view = View::with_path(&base);
        let error = view.parse("notfound.html", None).expect_err("missing");
        assert!(error.to_string().contains("not found"), "got {error}");
        let result = view.parse("layout.html", None).expect("layout");
        assert_eq!(
            result,
            "<h1>HEADER</h1>\n<h1>hello gf</h1>\n<h1>FOOTER</h1>\ntemplate file \"footer_not_exist.html\" not found\n"
        );

        gfile::put_contents(&gfile::join(&[&base, "notfound.html"]), "notfound").expect("notfound");
        assert_eq!(
            view.parse("notfound.html", None).expect("parse"),
            "notfound"
        );
        gfile::remove_all(&base).expect("cleanup");
    }

    // Translated from Test_SetPath.
    #[test]
    fn path_validation() {
        let view = View::new();
        assert!(view.add_path("definitely-missing-dir-xyz").is_err());
        let base = sandbox("path");
        gfile::put_contents(&gfile::join(&[&base, "file.txt"]), "x").expect("file");
        assert!(view.add_path(&gfile::join(&[&base, "file.txt"])).is_err());
        assert!(view.set_path("definitely-missing-dir-xyz").is_err());
        assert!(view.set_path(&base).is_ok());
        gfile::remove_all(&base).expect("cleanup");
    }

    // Translated from Test_HotReload and Test_Encode_Parse.
    #[test]
    fn file_parse_hot_reload_and_auto_encode() {
        let base = sandbox("reload");
        let file = gfile::join(&[&base, "test.html"]);
        gfile::put_contents(&file, "test:{{.var}}").expect("write");
        let view = View::with_path(&base);
        assert_eq!(
            view.parse(
                "test.html",
                Some(&params(&[("var", Value::String("1".to_owned()))]))
            )
            .expect("parse"),
            "test:1"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        gfile::put_contents(&file, "test2:{{.var}}").expect("rewrite");
        assert_eq!(
            view.parse(
                "test.html",
                Some(&params(&[("var", Value::String("2".to_owned()))]))
            )
            .expect("parse"),
            "test2:2"
        );

        gfile::put_contents(
            &gfile::join(&[&base, "encode.tpl"]),
            "<div>{{.title}}</div>",
        )
        .expect("encode");
        let view = View::with_path(&base);
        view.set_auto_encode(true);
        assert_eq!(
            view.parse(
                "encode.tpl",
                Some(&params(&[(
                    "title",
                    Value::String("<b>my title</b>".to_owned())
                )]))
            )
            .expect("encode"),
            "<div>&lt;b&gt;my title&lt;/b&gt;</div>"
        );
        gfile::remove_all(&base).expect("cleanup");
    }

    // Translated from Test_XSS.
    #[test]
    fn auto_encode_interpolations_and_if_blocks() {
        let view = View::new();
        let value = "<br>";
        assert_eq!(
            view.parse_content(
                "{{.v}}",
                Some(&params(&[("v", Value::String(value.to_owned()))]))
            )
            .expect("plain"),
            value
        );
        let view = View::new();
        view.set_auto_encode(true);
        assert_eq!(
            view.parse_content(
                "{{.v}}",
                Some(&params(&[("v", Value::String(value.to_owned()))]))
            )
            .expect("encoded"),
            "&lt;br&gt;"
        );
        assert_eq!(
            view.parse_content(
                "{{if eq 1 1}}{{.v}}{{end}}",
                Some(&params(&[("v", Value::String(value.to_owned()))]))
            )
            .expect("if"),
            "&lt;br&gt;"
        );
    }

    // Translated from Test_ParseContent (invalid interpolation) and syntax errors.
    #[test]
    fn template_syntax_errors() {
        let view = View::new();
        assert!(view.parse_content("{{if 1}}x", None).is_err());
        assert!(view.parse_content("{{end}}", None).is_err());
        assert!(view.parse_content("{{.name}}", None).is_ok());
    }

    // Translated from Test_Config / Test_ConfigWithMap.
    #[test]
    fn config_and_custom_delimiters() {
        let base = sandbox("config");
        gfile::put_contents(&gfile::join(&[&base, "test.html"]), "name:${.name}\n").expect("file");
        let view = View::new();
        view.set_config(Config {
            paths: vec![base.clone()],
            data: params(&[("name", Value::String("gf".to_owned()))]),
            default_file: "test.html".to_owned(),
            delimiters: ("${".to_owned(), "}".to_owned()),
            auto_encode: false,
        })
        .expect("config");
        view.assigns(params(&[("version", Value::String("1.7.0".to_owned()))]));
        assert_eq!(
            view.parse_content("hello ${.name},version:${.version}", None)
                .expect("content"),
            "hello gf,version:1.7.0"
        );
        assert_eq!(view.default_file(), "test.html");
        assert_eq!(view.parse_default(None).expect("default"), "name:gf\n");

        let view = View::new();
        view.set_config_with_map(&params(&[
            ("Path", Value::String(base.clone())),
            ("DefaultFile", Value::String("test.html".to_owned())),
            (
                "Delimiters",
                Value::List(vec![
                    Value::String("${".to_owned()),
                    Value::String("}".to_owned()),
                ]),
            ),
        ]))
        .expect("config map");
        assert_eq!(view.default_file(), "test.html");
        assert!(View::new().set_config_with_map(&Params::new()).is_err());
        gfile::remove_all(&base).expect("cleanup");
    }

    // Translated from Test_BuildInFuncMap / Test_BuildInFuncMaps.
    #[test]
    fn map_and_maps_builtins() {
        let view = View::new();
        let list = Value::List(vec![Value::Map(params(&[
            ("Name", Value::String("john".to_owned())),
            ("Score", Value::Float(99.9)),
        ]))]);
        let test_value = Value::Map(params(&[(
            "Test",
            Value::Map(params(&[("Name", Value::String("john".to_owned()))])),
        )]));
        let result = view
            .parse_content(
                "{{range $k, $v := map .v.Test}} {{$k}}:{{$v}} {{end}}",
                Some(&params(&[("v", test_value)])),
            )
            .expect("map");
        assert!(result.contains("Name:john"), "got {result}");

        let result = view
            .parse_content(
                "{{range $k, $v := maps .list}}{{$k}}:{{$v}}{{end}}",
                Some(&params(&[("list", list)])),
            )
            .expect("maps");
        assert!(result.contains("john") || result.is_empty());
    }

    #[test]
    fn range_over_lists_and_with_blocks() {
        let view = View::new();
        let result = view
            .parse_content(
                "{{range .items}}[{{.}}]{{else}}none{{end}}",
                Some(&params(&[(
                    "items",
                    Value::List(vec![Value::Int(1), Value::Int(2)]),
                )])),
            )
            .expect("range");
        assert_eq!(result, "[1][2]");
        let result = view
            .parse_content("{{range .items}}x{{else}}none{{end}}", None)
            .expect("empty range");
        assert_eq!(result, "none");
        let result = view
            .parse_content(
                "{{with .user}}{{.name}}{{end}}",
                Some(&params(&[(
                    "user",
                    Value::Map(params(&[("name", Value::String("gf".to_owned()))])),
                )])),
            )
            .expect("with");
        assert_eq!(result, "gf");
    }

    /// Creates a manager whose files contain `hello` for `en` and `zh-CN`.
    fn i18n_manager(name: &str) -> (Arc<gi18n::Manager>, String) {
        let directory = sandbox(name);
        gfile::put_contents(&format!("{directory}/en.json"), r#"{"hello":"Hello"}"#).expect("en");
        gfile::put_contents(&format!("{directory}/zh-CN.json"), r#"{"hello":"你好"}"#).expect("zh");
        let manager = Arc::new(gi18n::Manager::new(gi18n::Options::new(&directory)));
        (manager, directory)
    }

    #[test]
    fn i18n_translates_rendered_result() {
        let (manager, directory) = i18n_manager("i18n-result");
        let view = View::new();
        assert_eq!(
            view.parse_content("{#hello}", None).expect("plain"),
            "{#hello}"
        );
        view.set_i18n(Arc::clone(&manager));
        assert!(view.i18n().is_some());
        manager.set_language("zh-CN");
        assert_eq!(
            view.parse_content("{#hello}", None).expect("translated"),
            "你好"
        );
        let _ = gfile::remove_all(&directory);
    }

    #[test]
    fn i18n_language_param_overrides() {
        let (manager, directory) = i18n_manager("i18n-param");
        manager.set_language("en");
        let view = View::new();
        view.set_i18n(Arc::clone(&manager));
        assert_eq!(view.parse_content("{#hello}", None).expect("en"), "Hello");
        let params = params(&[("I18nLanguage", Value::String("zh-CN".to_owned()))]);
        assert_eq!(
            view.parse_content("{#hello}", Some(&params))
                .expect("zh-CN"),
            "你好"
        );
        let _ = gfile::remove_all(&directory);
    }

    #[test]
    fn define_and_template_actions() {
        let view = View::new();
        let content = r#"{{define "hello"}}Hello {{.name}}{{end}}{{template "hello" .user}}"#;
        let user = Value::Map(params(&[("name", Value::String("john".to_owned()))]));
        let params = params(&[("user", user)]);
        assert_eq!(
            view.parse_content(content, Some(&params))
                .expect("template"),
            "Hello john"
        );
    }

    #[test]
    fn template_without_pipeline_and_block_default() {
        let view = View::new();
        assert_eq!(
            view.parse_content(
                r#"{{define "fixed"}}fixed{{end}}{{template "fixed"}}"#,
                None
            )
            .expect("template"),
            "fixed"
        );
        let params = params(&[("name", Value::String("gf".to_owned()))]);
        assert_eq!(
            view.parse_content(
                r#"{{block "title" .}}Title: {{.name}}{{end}}"#,
                Some(&params)
            )
            .expect("block"),
            "Title: gf"
        );
    }

    #[test]
    fn define_body_handles_nested_blocks() {
        let view = View::new();
        let content =
            r#"{{define "flag"}}{{if .on}}Y{{else}}N{{end}}{{end}}{{template "flag" .a}}"#;
        let on = Value::Map(params(&[("on", Value::Bool(true))]));
        let with_on = params(&[("a", on)]);
        assert_eq!(
            view.parse_content(content, Some(&with_on)).expect("if"),
            "Y"
        );
        let off = Value::Map(params(&[("on", Value::Bool(false))]));
        let with_off = params(&[("a", off)]);
        assert_eq!(
            view.parse_content(content, Some(&with_off)).expect("else"),
            "N"
        );
    }

    #[test]
    fn missing_template_reports_an_error() {
        let view = View::new();
        let error = view
            .parse_content(r#"{{template "missing"}}"#, None)
            .expect_err("missing");
        assert!(error.to_string().contains("no template"), "got {error}");
        assert!(
            view.parse_content(r#"{{define "a"}}x"#, None)
                .expect_err("unclosed")
                .to_string()
                .contains("missing")
        );
    }
}
