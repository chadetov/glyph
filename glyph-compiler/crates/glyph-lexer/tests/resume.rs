//! The pull lexer against the eager one, over every `.glyph` file in the
//! repository.
//!
//! The parser lexes on demand and will resume the lexer mid-file, so two
//! properties have to hold on real programs, not only on the small cases in
//! the unit tests: pulling `next_token` to EOF gives exactly what `tokenize`
//! gives, and resuming with `Lexer::at` from the end of any token, in the
//! state saved after it, gives exactly the rest of that stream.

use std::path::{Path, PathBuf};

use glyph_lexer::{tokenize, LexError, LexState, Lexer, Spanned, Token};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn glyph_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            glyph_files(&path, out);
        } else if name.ends_with(".glyph") {
            out.push(path);
        }
    }
}

/// Pull to EOF or the first error, keeping the state after every token.
fn pull(lexer: &mut Lexer<'_>) -> Result<Vec<(Spanned<Token>, LexState)>, LexError> {
    let mut out = Vec::new();
    loop {
        let t = lexer.next_token()?;
        let eof = matches!(t.token, Token::Eof);
        out.push((t, lexer.state()));
        if eof {
            return Ok(out);
        }
    }
}

#[test]
fn pulling_and_resuming_match_tokenize_on_every_corpus_file() {
    let mut files = Vec::new();
    glyph_files(&repo_root(), &mut files);
    files.sort();
    assert!(files.len() > 100, "corpus not found: {} files", files.len());

    let mut resumes = 0usize;
    for file in &files {
        let Ok(src) = std::fs::read_to_string(file) else {
            continue;
        };
        let eager = tokenize(&src);
        let pulled = pull(&mut Lexer::new(&src));
        let (eager, pulled) = match (eager, pulled) {
            (Ok(e), Ok(p)) => (e, p),
            (Err(e), Err(p)) => {
                assert_eq!(e.to_string(), p.to_string(), "{}", file.display());
                assert_eq!(e.span(), p.span(), "{}", file.display());
                continue;
            }
            (e, p) => panic!(
                "{}: eager {:?} but pulled {:?}",
                file.display(),
                e.map(|v| v.len()),
                p.map(|v| v.len())
            ),
        };
        let tokens: Vec<_> = pulled.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(tokens, eager, "{}", file.display());

        // Resume after every token. Each resume lexes the rest of the file,
        // so stride through the long ones to keep the test quick while still
        // landing inside brackets, strings' neighbours and line ends.
        let stride = (eager.len() / 200).max(1);
        for i in (0..eager.len()).step_by(stride) {
            let (tok, state) = &pulled[i];
            let mut resumed = Lexer::at(&src, tok.span.end as usize, *state);
            let rest = pull(&mut resumed).expect("a clean file stays clean");
            let rest: Vec<_> = rest.into_iter().map(|(t, _)| t).collect();
            assert_eq!(
                rest,
                eager[i + 1..].iter().cloned().chain(
                    // Resuming after EOF yields EOF again.
                    (i + 1 == eager.len()).then(|| eager[i].clone())
                ).collect::<Vec<_>>(),
                "{}: resume after token {i} ({:?})",
                file.display(),
                tok
            );
            resumes += 1;
        }
    }
    assert!(resumes > 1000, "only {resumes} resumes ran");
}
