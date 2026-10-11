//! A refused record names the fields it lacks, and a component element is
//! asked for the props it does not write.
//!
//! G232 gave an object literal a record type and G240 checked JSX attributes
//! against a component's props record, and both stopped one sentence short.
//! `return { name: "a" }` under `-> User` drew ``expected `User`, found
//! `record` `` with nothing saying `age` was the problem, where `tsc` says
//! `Property 'age' is missing`. And `<Card title="x" />` with a required
//! `count` drew nothing at all from `glyph check --no-tsc`: only `tsc`, on the
//! emitted `React.createElement(Card, { title: "x" })`, refused it.
//!
//! Every refusal below was established under `tsc --strict` one shape at a
//! time, by writing the TypeScript the emitter produces and running it; the
//! `tsc` error is recorded on each entry. Every acceptance below was
//! established the same way and compiles there.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn unique_tmp(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "glyph_missing_fields_{prefix}_{}_{}",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("create temp dir");
    dir
}

fn stage(name: &str, source: &str) -> PathBuf {
    let root = unique_tmp(name);
    std::fs::write(
        root.join("package.json"),
        format!("{{ \"name\": \"{name}\", \"version\": \"0.0.0\", \"glyph\": {{}} }}\n"),
    )
    .expect("write package.json");
    std::fs::write(root.join("src").join("main.glyph"), source).expect("write module");
    root
}

/// `glyph check --no-tsc --no-test --json` on `source`, parsed.
fn check_json(name: &str, source: &str) -> serde_json::Value {
    let root = stage(name, source);
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(["check", ".", "--no-tsc", "--no-test", "--json"])
        .current_dir(&root)
        .output()
        .expect("run glyph check");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&root);
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: not JSON ({e}): {text}"))
}

fn diagnostics(v: &serde_json::Value) -> Vec<serde_json::Value> {
    v["diagnostics"].as_array().expect("diagnostics array").clone()
}

/// The errors (not the warning-tier lints) in `v`.
fn errors(v: &serde_json::Value) -> Vec<serde_json::Value> {
    diagnostics(v)
        .into_iter()
        .filter(|d| d["severity"] == "error")
        .collect()
}

const RECORDS: &str = "module main\n\
\n\
import std/option { Option, Some, None }\n\
import std/nullable\n\
\n\
type User = { name: string, age: number, nick?: string, tag: Option<string>, seen: Nullable<number> }\n\
\n\
interface Named {\n\
\x20 name: string\n\
\x20 id: number\n\
}\n\
\n\
type Order = { user: User, total: number }\n\
\n\
type Box<T> = { value: T, label: string }\n\
\n\
fn takes(u: User) -> number {\n\
\x20 return u.age\n\
}\n\
\n\
fn use_some() -> Option<string> {\n\
\x20 return Some(\"x\")\n\
}\n";

/// A record shape that must be refused, with the code, the missing list and
/// the `tsc --strict` error the emitted TypeScript drew.
struct Refused {
    name: &'static str,
    body: &'static str,
    code: &'static str,
    missing: &'static [&'static str],
    #[allow(dead_code)]
    tsc: &'static str,
}

const REFUSED: &[Refused] = &[
    Refused {
        name: "let",
        body: "fn main() -> number {\n  let u: User = { name: \"a\", tag: None, seen: nullable.from_option(None) }\n  return u.age\n}\n",
        code: "E0204",
        missing: &["age"],
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; tag: Option<never>; seen: null; }' but required in type 'User'.",
    },
    Refused {
        name: "const",
        body: "const U0: User = { name: \"a\", tag: None, seen: nullable.from_option(None) }\n\
               fn main() -> number {\n  return U0.age\n}\n",
        code: "E0204",
        missing: &["age"],
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; tag: Option<never>; seen: null; }' but required in type 'User'.",
    },
    Refused {
        name: "return",
        body: "fn make() -> User {\n  return { name: \"a\", tag: None, seen: nullable.from_option(None) }\n}\n\
               fn main() -> number {\n  return make().age\n}\n",
        code: "E0204",
        missing: &["age"],
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; tag: Option<never>; seen: null; }' but required in type 'User'.",
    },
    Refused {
        name: "argument",
        body: "fn main() -> number {\n  return takes({ name: \"a\", tag: Some(\"x\"), seen: nullable.from_option(None) })\n}\n",
        code: "E0211",
        missing: &["age"],
        tsc: "TS2345: Argument of type '{ name: string; tag: Option<string>; seen: number; }' is not assignable to parameter of type 'User'. Property 'age' is missing.",
    },
    // The recursion into a written field reaches the inner literal, and the
    // refusal is the inner one, underlined there.
    Refused {
        name: "nested_field",
        body: "fn main() -> number {\n  let o: Order = { user: { name: \"a\", tag: None, seen: nullable.from_option(None) }, total: 2 }\n  return o.total\n}\n",
        code: "E0204",
        missing: &["age"],
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; tag: Option<never>; seen: null; }' but required in type 'User'.",
    },
    Refused {
        name: "nullable_of_record",
        body: "fn main() -> number {\n  let n: Nullable<User> = { name: \"a\", tag: None, seen: nullable.from_option(None) }\n  return 0\n}\n",
        code: "E0204",
        missing: &["age"],
        tsc: "TS2741: Property 'age' is missing in type '{ name: string; tag: Option<never>; seen: null; }' but required in type 'User'.",
    },
    Refused {
        name: "interface",
        body: "fn main() -> number {\n  let i: Named = { name: \"x\" }\n  return i.id\n}\n",
        code: "E0204",
        missing: &["id"],
        tsc: "TS2741: Property 'id' is missing in type '{ name: string; }' but required in type 'Named'.",
    },
    Refused {
        name: "generic_record",
        body: "fn main() -> number {\n  let b: Box<number> = { value: 1 }\n  return b.value\n}\n",
        code: "E0204",
        missing: &["label"],
        tsc: "TS2741: Property 'label' is missing in type '{ value: number; }' but required in type 'Box<number>'.",
    },
    // An `Option<T>` field and a `Nullable<T>` field are required: `?` is the
    // only spelling of an optional field, in Glyph and in what it emits.
    Refused {
        name: "option_field_omitted",
        body: "fn main() -> number {\n  let u: User = { name: \"a\", age: 1, seen: nullable.from_option(None) }\n  return u.age\n}\n",
        code: "E0204",
        missing: &["tag"],
        tsc: "TS2741: Property 'tag' is missing in type '{ name: string; age: number; seen: null; }' but required in type 'User'.",
    },
    Refused {
        name: "nullable_field_omitted",
        body: "fn main() -> number {\n  let u: User = { name: \"a\", age: 1, tag: None }\n  return u.age\n}\n",
        code: "E0204",
        missing: &["seen"],
        tsc: "TS2741: Property 'seen' is missing in type '{ name: string; age: number; tag: Option<never>; }' but required in type 'User'.",
    },
    Refused {
        name: "two_missing",
        body: "fn main() -> number {\n  let u: User = { name: \"a\", seen: nullable.from_option(None) }\n  return u.age\n}\n",
        code: "E0204",
        missing: &["age", "tag"],
        tsc: "TS2739: Type '{ name: string; seen: null; }' is missing the following properties from type 'User': age, tag",
    },
    // A value that is not a literal but has a structural record type is the
    // same pairing.
    Refused {
        name: "structural_binding",
        body: "fn main() -> number {\n  let a: { name: string, age: number } = { name: \"a\", age: 1 }\n  let u: User = a\n  return u.age\n}\n",
        code: "E0204",
        missing: &["tag", "seen"],
        tsc: "TS2739: Type '{ name: string; age: number; }' is missing the following properties from type 'User': tag, seen",
    },
];

/// The sentence ends with the missing names, in the declared order.
fn suffix(missing: &[&str]) -> String {
    let quoted: Vec<String> = missing.iter().map(|m| format!("`{m}`")).collect();
    match missing {
        [_] => format!(": missing field {}", quoted[0]),
        _ => format!(": missing fields {}", quoted.join(", ")),
    }
}

#[test]
fn every_record_site_names_the_missing_fields_in_the_sentence_and_the_json() {
    for case in REFUSED {
        let v = check_json(case.name, &format!("{RECORDS}\n{}", case.body));
        let errs = errors(&v);
        assert_eq!(errs.len(), 1, "{}: one error expected: {v:#}", case.name);
        let d = &errs[0];
        assert_eq!(d["code"], case.code, "{}: {d:#}", case.name);
        assert_eq!(d["actual"], "record", "{}: {d:#}", case.name);
        assert_eq!(
            d["missing"],
            serde_json::json!(case.missing),
            "{}: {d:#}",
            case.name
        );
        let message = d["message"].as_str().expect("message");
        assert!(
            message.ends_with(&suffix(case.missing)),
            "{}: {message}",
            case.name
        );
    }
}

/// None of these is missing a required field, so none is refused, and a
/// refusal that is not about a missing field says `null`.
#[test]
fn an_optional_field_or_an_extra_one_is_not_missing() {
    // `nick?:` is optional and left out; tsc accepts it.
    let optional = "fn main() -> number {\n  let u: User = { name: \"a\", age: 1, tag: None, seen: nullable.from_option(None) }\n  return u.age\n}\n";
    // An extra field through a binding is width subtyping; tsc accepts it.
    let extra = "fn main() -> number {\n  let w = { name: \"a\", age: 1, tag: None, seen: nullable.from_option(None), zzz: 1 }\n  let u: User = w\n  return u.age\n}\n";
    for (name, body) in [("optional", optional), ("extra", extra)] {
        let v = check_json(name, &format!("{RECORDS}\n{body}"));
        assert!(errors(&v).is_empty(), "{name}: {v:#}");
    }
}

#[test]
fn a_mismatch_that_is_not_about_a_missing_field_carries_null() {
    // A primitive refused against a primitive: the same code, no record.
    let body = "fn main() -> number {\n  let x: string = 1\n  return 0\n}\n";
    let v = check_json("unrelated", &format!("{RECORDS}\n{body}"));
    let errs = errors(&v);
    assert_eq!(errs.len(), 1, "{v:#}");
    assert_eq!(errs[0]["code"], "E0204");
    assert!(errs[0]["missing"].is_null(), "{:#}", errs[0]);
    assert!(errs[0].as_object().unwrap().contains_key("missing"));
    // And an unrelated code carries the key as an explicit null too.
    let typo = "fn main() -> number {\n  let u: User = { name: \"a\", age: 1, tag: None, seen: nullable.from_option(None) }\n  return u.agee\n}\n";
    let v = check_json("other_code", &format!("{RECORDS}\n{typo}"));
    let errs = errors(&v);
    assert_eq!(errs[0]["code"], "E0210", "{v:#}");
    assert!(errs[0].as_object().unwrap().contains_key("missing"));
    assert!(errs[0]["missing"].is_null(), "{:#}", errs[0]);
}

const COMPONENTS: &str = "module main\n\
\n\
type Props = { title: string, count: number, hint?: string }\n\
\n\
component Card(props: Props) {\n\
\x20 return <div>{props.title}</div>\n\
}\n\
\n\
component Kids(props: { title: string, children: string }) {\n\
\x20 return <div>{props.title}</div>\n\
}\n\
\n\
component OptKids(props: { title: string, children?: string }) {\n\
\x20 return <div>{props.title}</div>\n\
}\n\
\n\
component Bare() {\n\
\x20 return <div>x</div>\n\
}\n\
\n\
fn main() -> number {\n\
\x20 return 0\n\
}\n";

/// A component element and what it must draw.
struct Element {
    name: &'static str,
    app: &'static str,
    /// `None` for an element that is accepted.
    missing: Option<&'static [&'static str]>,
    #[allow(dead_code)]
    tsc: &'static str,
}

const ELEMENTS: &[Element] = &[
    Element {
        name: "one_missing",
        app: "  return <Card title=\"x\" />\n",
        missing: Some(&["count"]),
        tsc: "TS2769: Argument of type '{ title: string; }' is not assignable to parameter of type 'Attributes & Props'. Property 'count' is missing.",
    },
    Element {
        name: "two_missing",
        app: "  return <Card hint=\"h\" />\n",
        missing: Some(&["title", "count"]),
        tsc: "TS2769: Type '{ hint: string; }' is missing the following properties from type 'Props': title, count",
    },
    Element {
        name: "nested_in_intrinsic",
        app: "  return <div><Card title=\"x\" /></div>\n",
        missing: Some(&["count"]),
        tsc: "TS2769: Property 'count' is missing in type '{ title: string; }' but required in type 'Props'.",
    },
    // `key` is written into the props object, so the object exists.
    Element {
        name: "key_only",
        app: "  return <Card key=\"k\" />\n",
        missing: Some(&["title", "count"]),
        tsc: "TS2769: Type '{ key: string; }' is missing the following properties from type 'Props': title, count",
    },
    // Written children go to `createElement` as varargs, and `tsc` checks the
    // props object alone, so a required `children` field is still missing.
    Element {
        name: "required_children_written_as_children",
        app: "  return <Kids title=\"t\">hello</Kids>\n",
        missing: Some(&["children"]),
        tsc: "TS2769: Property 'children' is missing in type '{ title: string; }' but required in type '{ title: string; children: string; }'.",
    },
    Element {
        name: "required_children_as_attribute",
        app: "  return <Kids title=\"t\" children=\"hello\" />\n",
        missing: None,
        tsc: "accepted",
    },
    Element {
        name: "optional_children",
        app: "  return <OptKids title=\"t\">hello</OptKids>\n",
        missing: None,
        tsc: "accepted",
    },
    Element {
        name: "optional_prop_omitted",
        app: "  return <Card title=\"x\" count={1} />\n",
        missing: None,
        tsc: "accepted",
    },
    // A spread may carry anything, so nothing is asked next to one. tsc
    // refuses this particular spread, which is outside what Glyph decides.
    Element {
        name: "spread",
        app: "  return <Card {...props.p} />\n",
        missing: None,
        tsc: "TS2769 (not decided by Glyph)",
    },
    // The same next to a written attribute, so the props object exists and
    // only the spread keeps the element from being asked.
    Element {
        name: "spread_with_attribute",
        app: "  return <Card {...props.p} hint=\"h\" />\n",
        missing: None,
        tsc: "TS2769 (not decided by Glyph)",
    },
    // No attribute at all lowers to `createElement(Card, null)`, which React's
    // declaration accepts for any component.
    Element {
        name: "no_attributes",
        app: "  return <Card />\n",
        missing: None,
        tsc: "accepted",
    },
    // A component with no parameter takes no props.
    Element {
        name: "no_parameter",
        app: "  return <Bare title=\"t\" />\n",
        missing: None,
        tsc: "accepted",
    },
];

#[test]
fn a_component_element_is_asked_for_the_props_it_does_not_write() {
    for case in ELEMENTS {
        let src = format!(
            "{COMPONENTS}\ncomponent App(props: {{ p: {{ title: string }} }}) {{\n{}}}\n",
            case.app
        );
        let v = check_json(case.name, &src);
        let errs = errors(&v);
        match case.missing {
            None => assert!(errs.is_empty(), "{}: {v:#}", case.name),
            Some(missing) => {
                assert_eq!(errs.len(), 1, "{}: {v:#}", case.name);
                let d = &errs[0];
                assert_eq!(d["code"], "E0211", "{}: {d:#}", case.name);
                assert_eq!(d["actual"], "record", "{}: {d:#}", case.name);
                assert_eq!(d["missing"], serde_json::json!(missing), "{}: {d:#}", case.name);
                let message = d["message"].as_str().expect("message");
                assert!(message.ends_with(&suffix(missing)), "{}: {message}", case.name);
            }
        }
    }
}

/// The diagnostic sits on the element's name and names the props type the
/// component declares.
#[test]
fn the_element_refusal_is_at_the_name_and_expects_the_props_type() {
    let src = format!(
        "{COMPONENTS}\ncomponent App(props: {{ x: string }}) {{\n  return <Card title=\"x\" />\n}}\n"
    );
    let v = check_json("at_name", &src);
    let errs = errors(&v);
    assert_eq!(errs.len(), 1, "{v:#}");
    let d = &errs[0];
    assert_eq!(d["expected"], "Props", "{d:#}");
    let start = d["range"]["start"]["offset"].as_u64().unwrap() as usize;
    let end = d["range"]["end"]["offset"].as_u64().unwrap() as usize;
    assert_eq!(&src[start..end], "Card", "{d:#}");
}

/// `--agent` serializes the same diagnostic, so the list is there too.
#[test]
fn the_agent_surface_carries_the_missing_list() {
    let src = format!(
        "{COMPONENTS}\ncomponent App(props: {{ x: string }}) {{\n  return <Card title=\"x\" />\n}}\n"
    );
    let root = stage("agent", &src);
    let out = Command::new(env!("CARGO_BIN_EXE_glyph"))
        .args(["check", ".", "--no-tsc", "--no-test", "--agent"])
        .current_dir(&root)
        .output()
        .expect("run glyph check --agent");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&root);
    let v: serde_json::Value = serde_json::from_str(&text).expect("--agent is JSON");
    let d = diagnostics(&v)
        .into_iter()
        .find(|d| d["code"] == "E0211")
        .unwrap_or_else(|| panic!("no E0211: {text}"));
    assert_eq!(d["missing"], serde_json::json!(["count"]), "{d:#}");
}
