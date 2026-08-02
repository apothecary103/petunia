//! Maths written as LaTeX, drawn as text.
//!
//! Signal has no maths in its protocol and no client that renders any, so
//! `$x^2$` travels as the six characters that were typed and arrives everywhere
//! else as the six characters that were typed. That is the constraint this is
//! built around: what petunia draws has to be a *reading* of the source, not a
//! substitution for it, because the person on the other end is reading the
//! source.
//!
//! What is here is a reading in two forms, from one grammar. `parse` gives the
//! tree — a fraction is a numerator, a denominator and the fact that one goes
//! over the other — and the view lays that out with real boxes: a rule between
//! the two, limits above and below a ∑, a radical with a bar over what is under
//! it, scripts set smaller and raised. `flatten` writes the same tree back out
//! as one line, for the places that can only take one: a notification, a
//! sidebar preview, a search index, and any equation set inside a sentence,
//! where an element per span is an element per line break.
//!
//! There is still no maths font and no glyph variants, so a bracket does not
//! grow to fit what it holds and an integral sign is the size it is. The
//! structure is drawn; the glyphs are Unicode's.
//!
//! Anything it cannot read comes back as close to what was typed as it can
//! manage rather than as an error: a message is not a document that failed to
//! compile.

/// Whether the maths was written between one dollar sign or two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Inline,
    /// `$$…$$`, which every maths dialect sets on a line of its own.
    Display,
}

/// A stretch of maths in a body, delimiters included, so a caller replacing one
/// knows exactly what to cut out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
    /// What was between the delimiters.
    pub tex: String,
}

/// Every stretch of maths in the text, in order and never overlapping.
///
/// A dollar sign is a currency symbol far more often than it is a delimiter, so
/// an opener only counts when a closer of the same kind follows it with
/// something between them that looks like maths rather than like prose: no line
/// break, and no space against either delimiter. `$5 and $6` therefore stays
/// what it says, while `$a + b$` does not.
pub fn spans(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;

    while at < bytes.len() {
        // An escaped dollar is a dollar, and it is skipped whole so the one
        // after it does not become an opener.
        if bytes[at] == b'\\' {
            at += next_char(text, at + 1);
            continue;
        }
        if bytes[at] != b'$' {
            at += next_char(text, at);
            continue;
        }

        let (kind, delimiter) = match bytes.get(at + 1) {
            Some(b'$') => (Kind::Display, "$$"),
            _ => (Kind::Inline, "$"),
        };
        let opens = at + delimiter.len();

        match closer(text, opens, delimiter).filter(|end| usable(&text[opens..*end], kind)) {
            Some(end) => {
                found.push(Span {
                    start: at,
                    end: end + delimiter.len(),
                    kind,
                    tex: text[opens..end].to_owned(),
                });
                at = end + delimiter.len();
            }
            None => at += delimiter.len(),
        }
    }

    found
}

/// Where the matching delimiter starts, skipping escaped ones.
fn closer(text: &str, from: usize, delimiter: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut at = from;

    while at < bytes.len() {
        if bytes[at] == b'\\' {
            at += 1 + next_char(text, at + 1);
            continue;
        }
        if text[at..].starts_with(delimiter) {
            return Some(at);
        }
        at += next_char(text, at);
    }
    None
}

/// The width of the character at `at`, or one past the end of the string.
fn next_char(text: &str, at: usize) -> usize {
    text[at.min(text.len())..]
        .chars()
        .next()
        .map_or(1, char::len_utf8)
}

/// Whether what is between two dollars is maths rather than a sentence that
/// happened to mention a price twice.
fn usable(tex: &str, kind: Kind) -> bool {
    if tex.is_empty() {
        return false;
    }
    // Display maths is a block and may run over several lines; inline maths
    // that wraps is prose with dollars in it.
    if kind == Kind::Inline && tex.contains('\n') {
        return false;
    }
    !tex.starts_with(char::is_whitespace) && !tex.ends_with(char::is_whitespace)
}

/// One piece of an equation. What the view lays out, and what `flatten` writes
/// back as a line.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// Characters set as they are. `slanted` has the variable rule already
    /// applied: a single letter is a quantity, a run of them is a word.
    Run { text: String, slanted: bool },
    /// Whatever was inside a pair of braces, which is one thing for the purpose
    /// of anything that attaches to it.
    Group(Vec<Node>),
    /// A numerator over a denominator.
    Frac(Vec<Node>, Vec<Node>),
    Sqrt(Vec<Node>),
    /// A base with whatever was raised or lowered against it.
    Script {
        base: Vec<Node>,
        over: Option<Vec<Node>>,
        under: Option<Vec<Node>>,
    },
    /// A ∑, a ∏ or a `lim`, whose limits go above and below it rather than
    /// beside it. Which is the difference TeX itself draws between these and an
    /// ordinary script, and the reason ∫ is not one of them.
    Big {
        glyph: String,
        over: Option<Vec<Node>>,
        under: Option<Vec<Node>>,
    },
    /// A `\left…\right` pair, which is the only bracket that knows what it is
    /// holding and can therefore be drawn to the height of it.
    Fenced {
        open: String,
        close: String,
        inner: Vec<Node>,
    },
    /// A combining mark over what it marks.
    Accent { mark: char, inner: Vec<Node> },
    /// A gap, in ordinary spaces.
    Space(usize),
    Break,
}

/// LaTeX as a tree.
pub fn parse(tex: &str) -> Vec<Node> {
    let mut rest = tex;
    sequence(&mut rest)
}

/// LaTeX as the nearest thing one line of Unicode has to it.
pub fn render(tex: &str) -> String {
    collapse(&flatten(&parse(tex)))
}

/// Everything up to the end, a closing brace, or the `\right` that ends a
/// fenced group. Neither terminator is consumed: whoever asked for the
/// sequence knows which one it was waiting for.
fn sequence(rest: &mut &str) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::new();

    while !rest.is_empty() && !rest.starts_with('}') && !rest.starts_with("\\right") {
        if let Some(after) = rest.strip_prefix('^') {
            *rest = after;
            let body = argument(rest);
            attach(&mut out, true, body);
            continue;
        }
        if let Some(after) = rest.strip_prefix('_') {
            *rest = after;
            let body = argument(rest);
            attach(&mut out, false, body);
            continue;
        }
        // A spacing command and an environment are read and come to nothing,
        // which is not the same as there being nothing left to read. Only an
        // item that consumed no input ends the sequence, and that is the one
        // thing that would otherwise spin.
        let before = rest.len();
        match atom(rest) {
            Some(node) => out.push(node),
            None if rest.len() == before => break,
            None => {}
        }
    }

    out
}

/// Hangs a script off whatever came before it. A second script on the same base
/// joins the first rather than replacing it, so `x^2_i` carries both.
fn attach(out: &mut Vec<Node>, high: bool, body: Vec<Node>) {
    let (over, under) = match high {
        true => (Some(body), None),
        false => (None, Some(body)),
    };

    match out.pop() {
        // A big operator takes its limits rather than scripts, and takes the
        // second one without losing the first.
        Some(Node::Big {
            glyph,
            over: was_over,
            under: was_under,
        }) => out.push(Node::Big {
            glyph,
            over: over.or(was_over),
            under: under.or(was_under),
        }),
        Some(Node::Script {
            base,
            over: was_over,
            under: was_under,
        }) => out.push(Node::Script {
            base,
            over: over.or(was_over),
            under: under.or(was_under),
        }),
        base => out.push(Node::Script {
            base: base.into_iter().collect(),
            over,
            under,
        }),
    }
}

/// One item, before anything is attached to it.
fn atom(rest: &mut &str) -> Option<Node> {
    let character = rest.chars().next()?;

    if character == '\\' {
        *rest = &rest[1..];
        return command(rest);
    }
    if character == '{' {
        *rest = &rest[1..];
        let inner = sequence(rest);
        *rest = rest.strip_prefix('}').unwrap_or(rest);
        return Some(Node::Group(inner));
    }
    // A closer where an item was expected is an argument that is not there:
    // `\frac{}{2}` has a numerator of nothing. Not consumed, because the
    // sequence that opened the group is the one that closes it.
    if character == '}' {
        return None;
    }
    if character.is_whitespace() || character == '&' {
        let width = rest
            .find(|c: char| !c.is_whitespace() && c != '&')
            .unwrap_or(rest.len());
        *rest = &rest[width..];
        return Some(Node::Space(1));
    }
    if character.is_ascii_alphabetic() {
        let width = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let text = rest[..width].to_owned();
        *rest = &rest[width..];
        // One letter is a quantity and is slanted; several are a word --
        // `sin`, `log`, whatever came out of `\text{…}` -- and TeX sets those
        // upright for the same reason.
        let slanted = text.chars().count() == 1;
        return Some(Node::Run { text, slanted });
    }
    if character.is_ascii_digit() {
        let width = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let text = rest[..width].to_owned();
        *rest = &rest[width..];
        return Some(Node::Run {
            text,
            slanted: false,
        });
    }

    *rest = &rest[character.len_utf8()..];
    Some(Node::Run {
        text: character.to_string(),
        slanted: false,
    })
}

/// Reads one command and whatever it takes. The backslash is already gone.
fn command(rest: &mut &str) -> Option<Node> {
    let end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());

    // A backslash before anything that is not a word is punctuation for the
    // compiler: `\\` breaks the line, the spacing commands are spacing, and an
    // escaped delimiter is the delimiter.
    if end == 0 {
        let character = rest.chars().next()?;
        *rest = &rest[character.len_utf8()..];
        return match character {
            '\\' => Some(Node::Break),
            ',' | ';' | ':' | '!' => None,
            other => Some(Node::Run {
                text: other.to_string(),
                slanted: false,
            }),
        };
    }

    let name = rest[..end].to_owned();
    *rest = &rest[end..];

    match name.as_str() {
        "frac" | "tfrac" | "dfrac" => {
            let numerator = argument(rest);
            let denominator = argument(rest);
            Some(Node::Frac(numerator, denominator))
        }
        "sqrt" => Some(Node::Sqrt(argument(rest))),
        // Words set upright, bold, blackboard or otherwise are just their
        // words: a message has one face for everything that is not code. Taken
        // raw, because what is in them is prose and reading it as maths would
        // slant the odd letter in the middle of a sentence.
        "text" | "mathrm" | "mathbf" | "mathit" | "mathbb" | "mathcal" | "operatorname" => {
            Some(Node::Run {
                text: raw(rest),
                slanted: false,
            })
        }
        "left" => {
            let open = fence(rest);
            let inner = sequence(rest);
            let close = match rest.strip_prefix("\\right") {
                Some(after) => {
                    *rest = after;
                    fence(rest)
                }
                None => String::new(),
            };
            Some(Node::Fenced { open, close, inner })
        }
        // A sizing hint in front of a delimiter that is already in the source.
        "big" | "Big" | "bigg" | "Bigg" => None,
        // An accent is a combining mark, which is the one piece of structure
        // Unicode can draw without a box: it goes *after* the character it sits
        // on, and only over one, because over a group it lands on the bracket.
        "vec" | "hat" | "bar" | "dot" | "ddot" | "tilde" | "overline" => Some(Node::Accent {
            mark: match name.as_str() {
                "vec" => '\u{20d7}',
                "hat" => '\u{0302}',
                "tilde" => '\u{0303}',
                "dot" => '\u{0307}',
                "ddot" => '\u{0308}',
                _ => '\u{0304}',
            },
            inner: argument(rest),
        }),
        // An environment is a layout instruction and there is no layout. The
        // name goes with it, so `\begin{matrix}` does not leave the word
        // "matrix" in the middle of the maths.
        "begin" | "end" => {
            let _ = argument(rest);
            None
        }
        "quad" | "qquad" => Some(Node::Space(2)),
        _ => match BIG.iter().find(|glyph| **glyph == name) {
            Some(_) => Some(Node::Big {
                glyph: symbol(&name),
                over: None,
                under: None,
            }),
            None => Some(Node::Run {
                text: symbol(&name),
                slanted: false,
            }),
        },
    }
}

/// What a command comes to, or its own name where nothing here knows it — the
/// backslash dropped, because it is punctuation for a compiler and there is no
/// compiler.
fn symbol(name: &str) -> String {
    SYMBOLS
        .iter()
        .find(|(command, _)| *command == name)
        .map(|(_, glyph)| (*glyph).to_owned())
        .unwrap_or_else(|| name.to_owned())
}

/// One argument: a braced group, a command, or the single character that
/// follows.
fn argument(rest: &mut &str) -> Vec<Node> {
    *rest = rest.trim_start_matches(' ');

    if rest.starts_with('{') {
        *rest = &rest[1..];
        let inner = sequence(rest);
        *rest = rest.strip_prefix('}').unwrap_or(rest);
        return inner;
    }

    atom(rest).into_iter().collect()
}

/// The text of a braced group, unread. What `\text{…}` holds is prose.
fn raw(rest: &mut &str) -> String {
    if !rest.starts_with('{') {
        return match atom(rest) {
            Some(node) => flatten(&[node]),
            None => String::new(),
        };
    }

    let inner = &rest[1..];
    let mut depth = 1;
    for (at, character) in inner.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            *rest = &inner[at + 1..];
            return inner[..at].to_owned();
        }
    }
    // An unclosed group takes the rest, which is what a reader does too.
    *rest = "";
    inner.to_owned()
}

/// The bracket a `\left` or a `\right` names. A full stop is TeX's way of
/// saying "no bracket at all", and it draws nothing here either.
fn fence(rest: &mut &str) -> String {
    *rest = rest.trim_start_matches(' ');
    let Some(character) = rest.chars().next() else {
        return String::new();
    };

    if character == '\\' {
        *rest = &rest[1..];
        return match command(rest) {
            Some(node) => flatten(&[node]),
            None => String::new(),
        };
    }

    *rest = &rest[character.len_utf8()..];
    match character {
        '.' => String::new(),
        other => other.to_string(),
    }
}

/// The tree as one line, which is what everything that is not the conversation
/// can take.
pub fn flatten(nodes: &[Node]) -> String {
    let mut out = String::new();

    for node in nodes {
        match node {
            Node::Run { text, .. } => out.push_str(text),
            Node::Group(inner) => out.push_str(&flatten(inner)),
            Node::Frac(numerator, denominator) => {
                out.push_str(&parenthesised(&flatten(numerator)));
                // The fraction slash, not the solidus: it is the one Unicode
                // has for exactly this and it kerns as a fraction rather than
                // as a divide.
                out.push('\u{2044}');
                out.push_str(&parenthesised(&flatten(denominator)));
            }
            Node::Sqrt(radicand) => {
                out.push('√');
                out.push_str(&parenthesised(&flatten(radicand)));
            }
            Node::Script { base, over, under } => {
                out.push_str(&flatten(base));
                if let Some(over) = over {
                    out.push_str(&raised(over, true));
                }
                if let Some(under) = under {
                    out.push_str(&raised(under, false));
                }
            }
            Node::Big { glyph, over, under } => {
                out.push_str(glyph);
                if let Some(under) = under {
                    out.push_str(&raised(under, false));
                }
                if let Some(over) = over {
                    out.push_str(&raised(over, true));
                }
            }
            Node::Fenced { open, close, inner } => {
                out.push_str(open);
                out.push_str(&flatten(inner));
                out.push_str(close);
            }
            Node::Accent { mark, inner } => {
                let inner = flatten(inner);
                out.push_str(&inner);
                if inner.chars().count() == 1 {
                    out.push(*mark);
                }
            }
            Node::Space(width) => out.push_str(&" ".repeat(*width)),
            Node::Break => out.push('\n'),
        }
    }

    out
}

/// A superscript or subscript in one line, in Unicode where every character of
/// it has a raised form.
///
/// `x^2` is x²; `x^{n+1}` has no `+` in the superscript block, so it stays
/// `x^(n+1)` — which is what somebody would have typed had they not been
/// writing LaTeX, and is unambiguous, where a half-raised `n` would not be.
fn raised(nodes: &[Node], high: bool) -> String {
    let body = flatten(nodes);
    let table = if high { SUPERSCRIPT } else { SUBSCRIPT };

    let shifted: Option<String> = body
        .chars()
        .map(|character| {
            table
                .iter()
                .find(|(plain, _)| *plain == character)
                .map(|(_, raised)| *raised)
        })
        .collect();

    match shifted {
        Some(shifted) => shifted,
        None => format!(
            "{}{}",
            if high { '^' } else { '_' },
            parenthesised(&body)
        ),
    }
}

/// The operators whose limits belong above and below them rather than beside.
/// An integral is deliberately not one: TeX sets its limits at the side, and so
/// does every book.
const BIG: &[&str] = &[
    "sum", "prod", "coprod", "bigcup", "bigcap", "bigoplus", "bigotimes", "lim", "limsup",
    "liminf", "max", "min", "sup", "inf",
];

/// Brackets around anything that is not already one thing, so `a⁄b+c` cannot be
/// read as the fraction it is not.
fn parenthesised(rendered: &str) -> String {
    let atomic = rendered.chars().count() <= 1
        || rendered.chars().all(|c| c.is_alphanumeric() || c == '.')
        || (rendered.starts_with('(') && rendered.ends_with(')'));

    match atomic {
        true => rendered.to_owned(),
        false => format!("({rendered})"),
    }
}

/// Which parts of a rendered equation are *variables*, as byte ranges into it.
///
/// Maths is not italic; a variable is. Setting the whole equation in italics —
/// which is what this did — slants the digits, the operators, the brackets and
/// the ∑, none of which is slanted in any book, and the result reads as a
/// sentence in italics that happens to contain symbols rather than as maths. So
/// the rule every typesetter uses: a single letter standing for a quantity is
/// italic, and everything else is upright.
///
/// Read off the rendered text rather than the source, because that is what is
/// being set and the source is gone by then. The classification is therefore by
/// shape:
///
/// - a run of one Latin letter is a variable, and a run of several is a word —
///   `sin`, `log`, and whatever came out of `\text{…}`, all of which TeX sets
///   upright for exactly this reason;
/// - lowercase Greek is a variable, and uppercase Greek is not, which is TeX's
///   own convention and the reason Σ and σ do not match in a paper;
/// - digits, operators, relations, brackets and everything else are upright.
pub fn variables(rendered: &str) -> Vec<std::ops::Range<usize>> {
    let mut found = Vec::new();
    let mut letters: Option<(usize, usize)> = None;

    for (at, character) in rendered.char_indices() {
        if character.is_ascii_alphabetic() {
            let (start, count) = letters.unwrap_or((at, 0));
            letters = Some((start, count + 1));
            continue;
        }
        // A run of one letter is a quantity; a run of several is a word.
        if let Some((start, 1)) = letters {
            found.push(start..at);
        }
        letters = None;

        if greek_variable(character) {
            found.push(at..at + character.len_utf8());
        }
    }

    if let Some((start, 1)) = letters {
        found.push(start..rendered.len());
    }
    found
}

/// Lowercase Greek, the variant letterforms included. Uppercase is left out on
/// purpose: TeX sets Γ and Σ upright and γ and σ italic, and a client that
/// slanted both would be wrong about half of them.
fn greek_variable(character: char) -> bool {
    matches!(character, '\u{03b1}'..='\u{03c9}' | 'ϑ' | 'ϕ' | 'ϖ' | 'ϰ' | 'ϱ')
}

/// LaTeX's own spacing is a compiler's business, and the source is full of the
/// newlines and runs of spaces that made it readable. None of that survives into
/// one line of a message.
fn collapse(rendered: &str) -> String {
    let mut out = String::with_capacity(rendered.len());
    let mut spaced = false;

    for character in rendered.chars() {
        match character {
            ' ' | '\t' => spaced = true,
            _ => {
                if spaced && !out.is_empty() && character != '\n' && !out.ends_with('\n') {
                    out.push(' ');
                }
                spaced = false;
                out.push(character);
            }
        }
    }
    out.trim().to_owned()
}

/// Digits, signs and the handful of letters Unicode has raised forms for.
const SUPERSCRIPT: &[(char, char)] = &[
    ('0', '⁰'), ('1', '¹'), ('2', '²'), ('3', '³'), ('4', '⁴'), ('5', '⁵'),
    ('6', '⁶'), ('7', '⁷'), ('8', '⁸'), ('9', '⁹'), ('+', '⁺'), ('-', '⁻'),
    ('=', '⁼'), ('(', '⁽'), (')', '⁾'), ('n', 'ⁿ'), ('i', 'ⁱ'),
];

const SUBSCRIPT: &[(char, char)] = &[
    ('0', '₀'), ('1', '₁'), ('2', '₂'), ('3', '₃'), ('4', '₄'), ('5', '₅'),
    ('6', '₆'), ('7', '₇'), ('8', '₈'), ('9', '₉'), ('+', '₊'), ('-', '₋'),
    ('=', '₌'), ('(', '₍'), (')', '₎'), ('a', 'ₐ'), ('e', 'ₑ'), ('h', 'ₕ'),
    ('i', 'ᵢ'), ('j', 'ⱼ'), ('k', 'ₖ'), ('l', 'ₗ'), ('m', 'ₘ'), ('n', 'ₙ'),
    ('o', 'ₒ'), ('p', 'ₚ'), ('r', 'ᵣ'), ('s', 'ₛ'), ('t', 'ₜ'), ('u', 'ᵤ'),
    ('v', 'ᵥ'), ('x', 'ₓ'),
];

/// What a command comes to. Every one of these is a glyph Unicode has and the
/// system font draws; nothing in here needs a maths font to be installed.
const SYMBOLS: &[(&str, &str)] = &[
    // Greek, lower then upper.
    ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"),
    ("epsilon", "ε"), ("varepsilon", "ε"), ("zeta", "ζ"), ("eta", "η"),
    ("theta", "θ"), ("vartheta", "ϑ"), ("iota", "ι"), ("kappa", "κ"),
    ("lambda", "λ"), ("mu", "μ"), ("nu", "ν"), ("xi", "ξ"), ("pi", "π"),
    ("varpi", "ϖ"), ("rho", "ρ"), ("varrho", "ϱ"), ("sigma", "σ"),
    ("varsigma", "ς"), ("tau", "τ"), ("upsilon", "υ"), ("phi", "φ"),
    ("varphi", "ϕ"), ("chi", "χ"), ("psi", "ψ"), ("omega", "ω"),
    ("Gamma", "Γ"), ("Delta", "Δ"), ("Theta", "Θ"), ("Lambda", "Λ"),
    ("Xi", "Ξ"), ("Pi", "Π"), ("Sigma", "Σ"), ("Upsilon", "Υ"),
    ("Phi", "Φ"), ("Psi", "Ψ"), ("Omega", "Ω"),
    // Operators and the big ones.
    ("times", "×"), ("div", "÷"), ("pm", "±"), ("mp", "∓"), ("cdot", "·"),
    ("cdots", "⋯"), ("ldots", "…"), ("dots", "…"), ("vdots", "⋮"),
    ("ast", "∗"), ("star", "⋆"), ("circ", "∘"), ("bullet", "∙"),
    ("sum", "∑"), ("prod", "∏"), ("coprod", "∐"), ("int", "∫"),
    ("iint", "∬"), ("iiint", "∭"), ("oint", "∮"), ("partial", "∂"),
    ("nabla", "∇"), ("infty", "∞"), ("surd", "√"), ("wedge", "∧"),
    ("vee", "∨"), ("cap", "∩"), ("cup", "∪"), ("oplus", "⊕"),
    ("otimes", "⊗"), ("setminus", "∖"), ("bigcup", "⋃"), ("bigcap", "⋂"),
    ("bigoplus", "⨁"), ("bigotimes", "⨂"), ("limsup", "lim sup"),
    ("liminf", "lim inf"),
    // Relations.
    ("leq", "≤"), ("le", "≤"), ("geq", "≥"), ("ge", "≥"), ("neq", "≠"),
    ("ne", "≠"), ("equiv", "≡"), ("approx", "≈"), ("sim", "∼"),
    ("simeq", "≃"), ("cong", "≅"), ("propto", "∝"), ("ll", "≪"),
    ("gg", "≫"), ("subset", "⊂"), ("subseteq", "⊆"), ("supset", "⊃"),
    ("supseteq", "⊇"), ("in", "∈"), ("notin", "∉"), ("ni", "∋"),
    ("perp", "⊥"), ("parallel", "∥"), ("mid", "∣"),
    // Arrows.
    ("to", "→"), ("rightarrow", "→"), ("longrightarrow", "⟶"),
    ("gets", "←"), ("leftarrow", "←"), ("longleftarrow", "⟵"),
    ("leftrightarrow", "↔"), ("Rightarrow", "⇒"), ("Leftarrow", "⇐"),
    ("Leftrightarrow", "⇔"), ("mapsto", "↦"), ("uparrow", "↑"),
    ("downarrow", "↓"), ("implies", "⟹"), ("iff", "⟺"),
    // Logic and sets.
    ("forall", "∀"), ("exists", "∃"), ("nexists", "∄"), ("neg", "¬"),
    ("lnot", "¬"), ("land", "∧"), ("lor", "∨"), ("emptyset", "∅"),
    ("varnothing", "∅"), ("therefore", "∴"), ("because", "∵"),
    ("aleph", "ℵ"), ("hbar", "ℏ"), ("ell", "ℓ"), ("Re", "ℜ"), ("Im", "ℑ"),
    // Named functions, which are set upright and otherwise unchanged.
    ("sin", "sin"), ("cos", "cos"), ("tan", "tan"), ("log", "log"),
    ("ln", "ln"), ("exp", "exp"), ("lim", "lim"), ("max", "max"),
    ("min", "min"), ("det", "det"), ("dim", "dim"), ("deg", "deg"),
    ("gcd", "gcd"), ("sup", "sup"), ("inf", "inf"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn only(text: &str) -> String {
        let found = spans(text);
        assert_eq!(found.len(), 1, "{text:?} -> {found:?}");
        render(&found[0].tex)
    }

    /// What comes back italicised, as the text of it, which is what the
    /// classification is actually about.
    fn slanted(rendered: &str) -> Vec<&str> {
        variables(rendered)
            .into_iter()
            .map(|variable| &rendered[variable])
            .collect()
    }

    /// The rule the whole thing rests on: a letter standing for a quantity is
    /// italic, and nothing else in the equation is.
    #[test]
    fn a_single_letter_is_a_variable_and_a_word_is_not() {
        assert_eq!(slanted("x"), vec!["x"]);
        assert_eq!(slanted("a + b"), vec!["a", "b"]);
        assert!(slanted("sin").is_empty());
        assert!(slanted("log x").contains(&"x"));
        assert!(!slanted("log x").contains(&"log"));
    }

    /// Digits, operators and brackets are upright in every book ever set, and
    /// slanting them is what made an equation read as an italicised sentence.
    #[test]
    fn nothing_but_letters_is_ever_slanted() {
        assert!(slanted("2 + 2 = 4").is_empty());
        assert!(slanted("∑ ∫ √ ∞ ≤ → ± ×").is_empty());
        assert!(slanted("(1 + 2)⁄3").is_empty());
    }

    /// TeX's own split, and the reason Σ and σ do not match in a paper.
    #[test]
    fn lowercase_greek_is_a_variable_and_uppercase_is_not() {
        assert_eq!(slanted("α + β"), vec!["α", "β"]);
        assert!(slanted("Σ Γ Δ Ω").is_empty());
    }

    /// The equations the module's own doc comment promises, read end to end.
    #[test]
    fn a_rendered_equation_slants_only_its_quantities() {
        assert_eq!(slanted(&only("$x^2 + 1$")), vec!["x"]);
        assert_eq!(slanted(&only("$\\frac{a}{b}$")), vec!["a", "b"]);
        assert_eq!(slanted(&only("$\\sum_{i} x$")), vec!["x"]);
    }

    #[test]
    fn a_symbol_is_its_glyph() {
        assert_eq!(only("$\\alpha + \\beta$"), "α + β");
        assert_eq!(only("$a \\leq b$"), "a ≤ b");
    }

    #[test]
    fn scripts_are_raised_where_unicode_can() {
        assert_eq!(only("$x^2$"), "x²");
        assert_eq!(only("$H_2O$"), "H₂O");
        assert_eq!(only("$x_i$"), "xᵢ");
    }

    /// A superscript Unicode has no glyph for stays legible instead of coming
    /// out half raised and half not.
    #[test]
    fn an_unraisable_script_keeps_its_marker() {
        assert_eq!(only("$x^{a+b}$"), "x^(a+b)");
    }

    #[test]
    fn a_fraction_is_a_slash_and_brackets_where_it_needs_them() {
        assert_eq!(only("$\\frac{1}{2}$"), "1⁄2");
        assert_eq!(only("$\\frac{a+b}{c}$"), "(a+b)⁄c");
    }

    #[test]
    fn a_root_takes_its_radicand() {
        assert_eq!(only("$\\sqrt{2}$"), "√2");
        assert_eq!(only("$\\sqrt{x+1}$"), "√(x+1)");
    }

    #[test]
    fn words_set_upright_are_their_words() {
        assert_eq!(only("$\\text{if } x > 0$"), "if x > 0");
    }

    /// The whole point of the delimiter rule: money is not maths.
    #[test]
    fn prices_are_not_delimiters() {
        assert!(spans("it was $5 and then $6").is_empty());
        assert!(spans("$ x $").is_empty());
        assert!(spans("costs \\$5").is_empty());
    }

    #[test]
    fn display_maths_is_told_from_inline() {
        let found = spans("before $$x^2$$ after");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Display);
        assert_eq!(found[0].tex, "x^2");
    }

    #[test]
    fn a_span_covers_its_delimiters() {
        let text = "a $b$ c";
        let found = spans(text);

        assert_eq!(&text[found[0].start..found[0].end], "$b$");
    }

    #[test]
    fn several_spans_do_not_overlap() {
        let found = spans("$a$ and $b$");

        assert_eq!(found.len(), 2);
        assert!(found[0].end <= found[1].start);
    }

    /// The tree is the point: a fraction that stays a numerator and a
    /// denominator is a fraction something can draw a rule between.
    #[test]
    fn a_fraction_survives_as_two_halves() {
        assert_eq!(
            parse("\\frac{a}{b+1}"),
            vec![Node::Frac(
                vec![Node::Run {
                    text: "a".into(),
                    slanted: true
                }],
                vec![
                    Node::Run {
                        text: "b".into(),
                        slanted: true
                    },
                    Node::Run {
                        text: "+".into(),
                        slanted: false
                    },
                    Node::Run {
                        text: "1".into(),
                        slanted: false
                    },
                ],
            )]
        );
    }

    /// A ∑ takes its limits above and below; an ∫ takes them beside, which is
    /// what TeX does and what every book does.
    #[test]
    fn only_the_operators_that_take_limits_take_limits() {
        assert!(matches!(
            parse("\\sum_{i=1}^{n}").as_slice(),
            [Node::Big {
                over: Some(_),
                under: Some(_),
                ..
            }]
        ));
        assert!(matches!(
            parse("\\int_0^1").as_slice(),
            [Node::Script { .. }]
        ));
    }

    /// A `\left…\right` pair knows what it is holding, which is what lets it be
    /// drawn to the height of it.
    #[test]
    fn a_left_right_pair_keeps_what_it_holds() {
        let Some(Node::Fenced { open, close, inner }) =
            parse("\\left(\\frac{1}{2}\\right)").into_iter().next()
        else {
            panic!("not a fenced group");
        };

        assert_eq!((open.as_str(), close.as_str()), ("(", ")"));
        assert!(matches!(inner.as_slice(), [Node::Frac(_, _)]));
    }

    /// A single letter is a quantity and slants; a run of them is a word and
    /// does not. The same rule `variables` applies to a rendered line.
    #[test]
    fn the_tree_carries_the_italics() {
        assert_eq!(
            parse("x"),
            vec![Node::Run {
                text: "x".into(),
                slanted: true
            }]
        );
        assert_eq!(
            parse("sin"),
            vec![Node::Run {
                text: "sin".into(),
                slanted: false
            }]
        );
    }

    /// An unknown command is a word, not a backslash and a word.
    #[test]
    fn an_unknown_command_loses_only_its_backslash() {
        assert_eq!(render("\\wobble x"), "wobble x");
    }

    #[test]
    fn sizing_hints_leave_the_delimiters_they_size() {
        assert_eq!(render("\\left( x \\right)"), "( x )");
    }

    #[test]
    fn a_line_break_survives() {
        assert_eq!(render("a \\\\ b"), "a\nb");
    }

    #[test]
    fn an_accent_becomes_a_combining_mark() {
        assert_eq!(only("$\\vec{v}$"), "v\u{20d7}");
        assert_eq!(only("$\\hat{x}$"), "x\u{0302}");
        // Over more than one character a combining mark lands on whichever
        // glyph happens to be last, so it is left off rather than misplaced.
        assert_eq!(only("$\\vec{ab}$"), "ab");
    }

    /// An environment is layout, and there is none. The name goes with it.
    #[test]
    fn an_environment_leaves_nothing_behind() {
        assert_eq!(render("\\begin{matrix} a \\end{matrix}"), "a");
        assert_eq!(only("$\\begin{cases} x \\end{cases}$"), "x");
    }

    /// Nesting has to survive: the inner group's closing brace must not be read
    /// as the outer one's.
    #[test]
    fn a_nested_group_is_matched_at_the_right_depth() {
        assert_eq!(only("$\\frac{\\frac{1}{2}}{3}$"), "(1⁄2)⁄3");
        assert_eq!(only("$x^{2}_{i}$"), "x²ᵢ");
    }

    /// The spacing commands are the compiler's and leave no character.
    #[test]
    fn spacing_commands_leave_no_glyph() {
        assert_eq!(only("$a\\,b$"), "ab");
        assert_eq!(only("$a\\;b\\!c$"), "abc");
        assert_eq!(only("$a\\quad b$"), "a b");
    }

    /// A dollar inside maths, escaped, is a dollar -- and must not close the
    /// span it is inside.
    #[test]
    fn an_escaped_dollar_does_not_close_the_span() {
        let found = spans("$a\\$b$");

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].tex, "a\\$b");
        assert_eq!(render(&found[0].tex), "a$b");
    }

    /// Display maths may run over lines; inline maths that does is prose with
    /// two dollars in it.
    #[test]
    fn only_display_maths_may_wrap() {
        assert!(spans("$a\n+b$").is_empty());
        assert_eq!(spans("$$a\n+b$$").len(), 1);
    }

    /// A multibyte character next to a delimiter is where a byte-indexed scanner
    /// slices through a character if it counts wrong.
    #[test]
    fn multibyte_characters_around_the_delimiters_are_safe() {
        let text = "héllo $\\alpha$ wörld";
        let found = spans(text);

        assert_eq!(found.len(), 1);
        assert_eq!(&text[found[0].start..found[0].end], "$\\alpha$");
        assert_eq!(only("$é^2$"), "é²");
    }

    /// Two spans on one line, with prose between them, all of it kept.
    #[test]
    fn spans_do_not_eat_what_is_between_them() {
        let text = "if $x > 0$ then $y = 1$ ok";
        let found = spans(text);

        assert_eq!(found.len(), 2);
        assert_eq!(&text[..found[0].start], "if ");
        assert_eq!(&text[found[0].end..found[1].start], " then ");
        assert_eq!(&text[found[1].end..], " ok");
    }

    /// Rendering must never panic on a multibyte character, an unclosed group or
    /// a trailing backslash, all of which arrive from a stranger.
    #[test]
    fn nothing_typeable_panics() {
        for input in [
            "\\", "^", "_", "{", "}", "\\frac", "\\frac{a", "\\sqrt", "é^2",
            "\\alpha{", "$$", "x_{", "\\text", "\\sqrt\\", "^\\", "$é$",
            "\\frac\\\\", "_{é", "$$é",
        ] {
            let _ = render(input);
            let _ = spans(input);
        }
    }
}
