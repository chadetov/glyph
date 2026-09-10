//! `glyph query <tool> ...` — one CLI verb per MCP tool.
//!
//! The MCP server answers nine questions about a project, and until this verb
//! existed reaching any of them meant speaking JSON-RPC over a pipe. An agent
//! without an MCP client had grep and the diagnostics, which is the surface
//! Glyph exists to improve on.
//!
//! Every verb routes through [`glyph_lsp::call_mcp_tool`], which is the same
//! `call_tool` an MCP client reaches. Nothing here decides anything about a
//! program: it reads flags, builds the tool's argument object, prints the JSON
//! the tool returned, and turns a refusal into an exit code. A flag the caller
//! did not pass is left out of the object rather than defaulted, so a tool's
//! own refusal for a missing argument is what the caller sees, in the words
//! the tool states it in.

use std::path::PathBuf;

use clap::Subcommand;
use serde_json::{json, Map, Value};

/// Exit code for a tool refusal: the request was read and not answered. `1` is
/// reserved for a program the compiler rejected, which is a different thing.
const REFUSED: i32 = 2;

#[derive(Subcommand)]
pub enum QueryCommand {
    /// Everything the compiler holds about one symbol: kind, identity,
    /// visibility, fields, variants with payloads and construction syntax,
    /// parameters and return, interface members, and whether a `match` over it
    /// must be exhaustive.
    Symbol {
        /// The symbol's `module::name` identity, or `module::Record.field`.
        #[arg(long, value_name = "MODULE::NAME")]
        entity: Option<String>,
        /// A `.glyph` file: with `--line`/`--character` it addresses a symbol
        /// by position, and with `--entity` it names which project to count
        /// that identity in.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
        /// 0-based line.
        #[arg(long)]
        line: Option<u32>,
        /// 0-based character (UTF-16 code units).
        #[arg(long)]
        character: Option<u32>,
    },
    /// Every diagnostic the compiler reports for one file, checked inside its
    /// project.
    Diagnostics {
        #[arg(long, value_name = "PATH")]
        path: String,
    },
    /// The type at a position.
    Hover {
        #[arg(long, value_name = "PATH")]
        path: String,
        #[arg(long)]
        line: u32,
        #[arg(long)]
        character: u32,
    },
    /// Where the name at a position is defined, following imports.
    Definition {
        #[arg(long, value_name = "PATH")]
        path: String,
        #[arg(long)]
        line: u32,
        #[arg(long)]
        character: u32,
    },
    /// Every edge into a symbol across the project, split by relation.
    References {
        #[arg(long, value_name = "PATH")]
        path: String,
        /// A top-level declaration, a variant, an import binding, or
        /// `Record.field`. Without it, address the symbol by position.
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        line: Option<u32>,
        #[arg(long)]
        character: Option<u32>,
        /// Narrow the answer to one relation.
        #[arg(long, value_name = "RELATION")]
        relation: Option<String>,
    },
    /// Every `match` site in the project over one tagged union, and which
    /// variants each site's arms name.
    Variants {
        #[arg(long, value_name = "PATH")]
        path: String,
        /// The union's name as the file at `--path` spells it.
        #[arg(long)]
        name: String,
        #[arg(long, value_name = "RELATION")]
        relation: Option<String>,
        /// Ask the change form: what adding this variant does to each site.
        #[arg(long, value_name = "VARIANT")]
        proposed_variant: Option<String>,
    },
    /// What breaks if you make one named change to one declaration.
    Impact {
        /// The declaration's `module::name`, or `module::Record.field`.
        #[arg(long, value_name = "MODULE::NAME")]
        entity: String,
        /// The change kind: `add_variant`, `remove_variant`, `rename`,
        /// `change_arity`, `change_signature_type`, `remove`.
        #[arg(long, value_name = "KIND")]
        change: String,
        /// The variant `add_variant` and `remove_variant` are about.
        #[arg(long, value_name = "NAME")]
        variant: Option<String>,
        /// Narrow to some of the change's carrier relations. Repeatable.
        #[arg(long, value_name = "RELATION")]
        relation: Vec<String>,
        /// Hops to answer. Defaults to 1.
        #[arg(long)]
        depth: Option<u32>,
        /// A file naming which project to count the identity in.
        #[arg(long, value_name = "PATH")]
        path: Option<String>,
    },
    /// Search the project's declarations by name substring.
    Symbols {
        /// Case-insensitive substring; empty matches everything.
        #[arg(long, default_value = "")]
        query: String,
    },
    /// Can a value of one type go where another is declared, asked of the
    /// checker's own comparison.
    Assignable {
        /// A `.glyph` file: the scope both types are read in.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// The value's type: `module::name`, or a Glyph type expression.
        #[arg(long, value_name = "TYPE")]
        from: String,
        /// The declared type the value would go into.
        #[arg(long, value_name = "TYPE")]
        to: String,
    },
}

/// Insert `key` only when the caller passed it, so a tool's own refusal for a
/// missing argument is what reaches the caller.
fn put<T: Into<Value>>(out: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(v) = value {
        out.insert(key.to_string(), v.into());
    }
}

impl QueryCommand {
    /// The MCP tool this verb asks.
    pub fn tool(&self) -> &'static str {
        match self {
            QueryCommand::Symbol { .. } => "glyph_symbol",
            QueryCommand::Diagnostics { .. } => "glyph_diagnostics",
            QueryCommand::Hover { .. } => "glyph_hover",
            QueryCommand::Definition { .. } => "glyph_definition",
            QueryCommand::References { .. } => "glyph_references",
            QueryCommand::Variants { .. } => "glyph_variants",
            QueryCommand::Impact { .. } => "glyph_impact",
            QueryCommand::Symbols { .. } => "glyph_symbols",
            QueryCommand::Assignable { .. } => "glyph_assignable",
        }
    }

    /// The tool's `arguments` object, exactly as an MCP client would send it.
    pub fn arguments(&self) -> Value {
        let mut out = Map::new();
        match self {
            QueryCommand::Symbol {
                entity,
                path,
                line,
                character,
            } => {
                put(&mut out, "entity", entity.clone());
                put(&mut out, "path", path.clone());
                put(&mut out, "line", *line);
                put(&mut out, "character", *character);
            }
            QueryCommand::Diagnostics { path } => {
                out.insert("path".to_string(), json!(path));
            }
            QueryCommand::Hover {
                path,
                line,
                character,
            }
            | QueryCommand::Definition {
                path,
                line,
                character,
            } => {
                out.insert("path".to_string(), json!(path));
                out.insert("line".to_string(), json!(line));
                out.insert("character".to_string(), json!(character));
            }
            QueryCommand::References {
                path,
                name,
                line,
                character,
                relation,
            } => {
                out.insert("path".to_string(), json!(path));
                put(&mut out, "name", name.clone());
                put(&mut out, "line", *line);
                put(&mut out, "character", *character);
                put(&mut out, "relation", relation.clone());
            }
            QueryCommand::Variants {
                path,
                name,
                relation,
                proposed_variant,
            } => {
                out.insert("path".to_string(), json!(path));
                out.insert("name".to_string(), json!(name));
                put(&mut out, "relation", relation.clone());
                put(&mut out, "proposed_variant", proposed_variant.clone());
            }
            QueryCommand::Impact {
                entity,
                change,
                variant,
                relation,
                depth,
                path,
            } => {
                out.insert("entity".to_string(), json!(entity));
                let mut kind = Map::new();
                kind.insert("kind".to_string(), json!(change));
                put(&mut kind, "variant", variant.clone());
                out.insert("change".to_string(), Value::Object(kind));
                if !relation.is_empty() {
                    out.insert("relations".to_string(), json!(relation));
                }
                put(&mut out, "depth", *depth);
                put(&mut out, "path", path.clone());
            }
            QueryCommand::Symbols { query } => {
                out.insert("query".to_string(), json!(query));
            }
            QueryCommand::Assignable { path, from, to } => {
                out.insert("path".to_string(), json!(path));
                out.insert("from".to_string(), json!(from));
                out.insert("to".to_string(), json!(to));
            }
        }
        Value::Object(out)
    }
}

/// Run one query. The tool's JSON goes to stdout and the exit code is 0; a
/// refusal goes to stderr with its reason and the exit code is 2.
pub fn run(root: Option<PathBuf>, command: &QueryCommand) -> i32 {
    let root = root
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    match glyph_lsp::call_mcp_tool(root, command.tool(), command.arguments()) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(why) => {
            eprintln!("glyph query {}: {why}", command.tool());
            REFUSED
        }
    }
}
