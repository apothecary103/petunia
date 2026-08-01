//! Maths written as LaTeX, drawn as text.
//!
//! Signal has no maths in its protocol and no client that renders any, so
//! `$x^2$` travels as the six characters that were typed and arrives everywhere
//! else as the six characters that were typed. That is the constraint this is
//! built around: what petunia draws has to be a *reading* of the source, not a
//! substitution for it, because the person on the other end is reading the
//! source.
//!
//! So there is no typesetting engine here. A fraction does not get a rule and a
//! numerator over a denominator, and an integral does not grow to fit its
//! bounds — that wants a box model and a maths font with the glyph variants to
//! fill it, which is a project rather than a module. What there is instead is
//! the transliteration everybody who writes maths in a chat window already does
//! by hand: `\alpha` is α, `x^2` is x², `H_2O` is H₂O, `\frac{a}{b}` is a⁄b. It
//! is exact for symbols, honest for scripts, and a compromise for structure —
//! and it beats both of the alternatives, which are the raw backslashes and a
//! blank.
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

/// LaTeX as the nearest thing Unicode has to it.
pub fn render(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len());
    let mut rest = tex;

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('\\') {
            rest = command(after, &mut out);
            continue;
        }

        let character = rest.chars().next().expect("rest is non-empty");
        match character {
            '^' | '_' => rest = script(&rest[1..], character == '^', &mut out),
            // A group's braces are structure, and the structure is gone by the
            // time this has been read: what is left is what was in them.
            '{' | '}' => rest = &rest[1..],
            // An alignment tab is a column break in a matrix nobody is drawing.
            '&' => {
                out.push(' ');
                rest = &rest[1..];
            }
            _ => {
                out.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }
    }

    collapse(&out)
}

/// Reads one command and whatever it takes, writing what it comes to.
fn command<'a>(after: &'a str, out: &mut String) -> &'a str {
    let end = after
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(after.len());

    // A backslash before anything that is not a word is punctuation for the
    // compiler: `\\` breaks the line, the spacing commands are spacing, and a
    // escaped delimiter is the delimiter.
    if end == 0 {
        let Some(character) = after.chars().next() else {
            return after;
        };
        match character {
            '\\' => out.push('\n'),
            ',' | ';' | ':' | '!' => {}
            other => out.push(other),
        }
        return &after[character.len_utf8()..];
    }

    let name = &after[..end];
    let rest = &after[end..];
    match name {
        "frac" | "tfrac" | "dfrac" => {
            let (numerator, rest) = argument(rest);
            let (denominator, rest) = argument(rest);
            out.push_str(&parenthesised(&numerator));
            // The fraction slash, not the solidus: it is the one Unicode has for
            // exactly this and it kerns as a fraction rather than as a divide.
            out.push('\u{2044}');
            out.push_str(&parenthesised(&denominator));
            rest
        }
        "sqrt" => {
            let (radicand, rest) = argument(rest);
            out.push('√');
            out.push_str(&parenthesised(&radicand));
            rest
        }
        // Words set upright, bold, blackboard or otherwise are just their words:
        // a message has one face for everything that is not code.
        "text" | "mathrm" | "mathbf" | "mathit" | "mathbb" | "mathcal"
        | "operatorname" => {
            let (words, rest) = argument(rest);
            out.push_str(&words);
            rest
        }
        // Sizing hints around a delimiter that is already in the source.
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" => rest,
        // An accent is a combining mark, which is the one piece of structure
        // Unicode can actually draw: it goes *after* the character it sits on.
        // Only over a single character, because a combining mark over a
        // parenthesised group lands on the bracket.
        "vec" | "hat" | "bar" | "dot" | "ddot" | "tilde" | "overline" => {
            let (under, rest) = argument(rest);
            out.push_str(&under);
            if under.chars().count() == 1 {
                out.push(match name {
                    "vec" => '\u{20d7}',
                    "hat" => '\u{0302}',
                    "tilde" => '\u{0303}',
                    "dot" => '\u{0307}',
                    "ddot" => '\u{0308}',
                    _ => '\u{0304}',
                });
            }
            rest
        }
        // An environment is a layout instruction, and there is no layout. The
        // name is consumed with it so `\begin{matrix}` does not leave the word
        // "matrix" in the middle of the maths.
        "begin" | "end" => argument(rest).1,
        "quad" | "qquad" => {
            out.push_str("  ");
            rest
        }
        _ => {
            match SYMBOLS.iter().find(|(command, _)| *command == name) {
                Some((_, glyph)) => out.push_str(glyph),
                // A command nothing here knows is written as its own name. The
                // backslash is dropped because it is punctuation for a compiler,
                // and there is no compiler.
                None => out.push_str(name),
            }
            rest
        }
    }
}

/// A superscript or subscript, in Unicode where every character of it has one.
///
/// `x^2` is x²; `x^{n+1}` has no `+` in the superscript block, so it stays
/// `x^(n+1)` — which is what somebody would have typed had they not been
/// writing LaTeX, and is unambiguous, where a half-raised `n` would not be.
fn script<'a>(rest: &'a str, high: bool, out: &mut String) -> &'a str {
    let (body, rest) = argument(rest);
    let table = if high { SUPERSCRIPT } else { SUBSCRIPT };

    let raised: Option<String> = body
        .chars()
        .map(|character| {
            table
                .iter()
                .find(|(plain, _)| *plain == character)
                .map(|(_, raised)| *raised)
        })
        .collect();

    match raised {
        Some(raised) => out.push_str(&raised),
        None => {
            out.push(if high { '^' } else { '_' });
            out.push_str(&parenthesised(&body));
        }
    }
    rest
}

/// One argument: a braced group, a command, or the single character that
/// follows. Returned rendered, so `\frac{\alpha}{2}` reads α⁄2.
fn argument(rest: &str) -> (String, &str) {
    let rest = rest.trim_start_matches(' ');

    if let Some(inner) = rest.strip_prefix('{') {
        let mut depth = 1;
        let mut at = 0;
        for (offset, character) in inner.char_indices() {
            match character {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                at = offset;
                break;
            }
        }
        // An unclosed group takes the rest, which is what a reader does too.
        if depth != 0 {
            return (render(inner), "");
        }
        return (render(&inner[..at]), &inner[at + 1..]);
    }

    // A command as an argument is the command and its name, and at the very
    // least the backslash: `\sqrt\` is nothing anybody meant, and slicing two
    // bytes out of one would take the process with it.
    if let Some(name) = rest.strip_prefix('\\') {
        let end = name
            .find(|c: char| !c.is_ascii_alphabetic())
            .map_or(rest.len(), |at| at + 1)
            .clamp(1, rest.len());
        return (render(&rest[..end]), &rest[end..]);
    }

    match rest.chars().next() {
        Some(character) => (
            render(&rest[..character.len_utf8()]),
            &rest[character.len_utf8()..],
        ),
        None => (String::new(), rest),
    }
}

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
    ("otimes", "⊗"), ("setminus", "∖"),
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
