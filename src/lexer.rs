//! Lexer for contingency-dsl: text -> token stream.
//!
//! Mirrors `contingency_dsl/lexer.py`. Two scanning modes share the same
//! character-level rules:
//!
//! * **Flat mode** — whitespace and newlines are skipped; the output is a
//!   dense sequence of schedule/annotation tokens ending in EOF.
//! * **Block mode** — activated when the source contains a whole-word
//!   `phase`, `progressive`, `shaping`, or `experiment` keyword. Adds
//!   COLON/NEWLINE/GTE/PERCENT recognition and emits one NEWLINE token at
//!   the end of every non-blank logical line.

use crate::enums::{
    is_kw_arg_keyword, is_schedule_type_keyword, COMBINATOR_KEYWORDS, DISTRIBUTION_NAMES,
    DR_KEYWORDS, EXPERIMENT_BLOCK_TRIGGERS, LOWERCASE_RESERVED, PR_PARAMS, SPECIAL_KEYWORDS,
};
use crate::errors::{DslError, Result, SourceSpan};
use crate::tokens::{Token, TokenType};

pub fn tokenize(source: &str) -> Result<Vec<Token>> {
    let block = is_block_mode(source);
    tokenize_inner(source, block)
}

pub fn is_block_mode(source: &str) -> bool {
    let decommented = strip_line_comments(source);
    let stripped = strip_string_literals(&decommented);
    for line in stripped.split('\n') {
        let trimmed = line.trim_start_matches([' ', '\t']);
        for trigger in EXPERIMENT_BLOCK_TRIGGERS {
            if let Some(rest) = trimmed.strip_prefix(trigger) {
                let boundary_ok = rest
                    .chars()
                    .next()
                    .map(|c| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(true);
                if boundary_ok {
                    return true;
                }
            }
        }
    }
    false
}

fn strip_line_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.split('\n') {
        let cut = line.find("--").unwrap_or(line.len());
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    if !src.ends_with('\n') && !out.is_empty() {
        out.pop();
    }
    out
}

fn strip_string_literals(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_string = false;
    for c in src.chars() {
        if c == '"' {
            out.push('"');
            in_string = !in_string;
        } else if !in_string {
            out.push(c);
        }
    }
    out
}

fn tokenize_inner(source: &str, block: bool) -> Result<Vec<Token>> {
    let mut tokens: Vec<Token> = Vec::new();
    let lines: Vec<Vec<char>> = source.split('\n').map(|s| s.chars().collect()).collect();
    let mut paren_depth: i32 = 0;

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = line_idx + 1;
        let mut col: usize = 0;
        let start_token_count = tokens.len();

        while col < line.len() {
            let ch = line[col];

            if ch == ' ' || ch == '\t' {
                col += 1;
                continue;
            }

            if ch == '-' && col + 1 < line.len() && line[col + 1] == '>' {
                tokens.push(Token::new(TokenType::Arrow, "->", line_num, col + 1));
                col += 2;
                continue;
            }

            if ch == '-' && col + 1 < line.len() && line[col + 1] == '-' {
                break;
            }

            if block && ch == '>' && col + 1 < line.len() && line[col + 1] == '=' {
                tokens.push(Token::new(TokenType::Gte, ">=", line_num, col + 1));
                col += 2;
                continue;
            }

            match ch {
                '(' => {
                    tokens.push(Token::new(TokenType::LParen, "(", line_num, col + 1));
                    paren_depth += 1;
                    col += 1;
                    continue;
                }
                ')' => {
                    tokens.push(Token::new(TokenType::RParen, ")", line_num, col + 1));
                    if paren_depth > 0 {
                        paren_depth -= 1;
                    }
                    col += 1;
                    continue;
                }
                ',' => {
                    tokens.push(Token::new(TokenType::Comma, ",", line_num, col + 1));
                    col += 1;
                    continue;
                }
                '=' => {
                    tokens.push(Token::new(TokenType::Equals, "=", line_num, col + 1));
                    col += 1;
                    continue;
                }
                '@' => {
                    tokens.push(Token::new(TokenType::At, "@", line_num, col + 1));
                    col += 1;
                    continue;
                }
                '[' => {
                    tokens.push(Token::new(TokenType::LBracket, "[", line_num, col + 1));
                    paren_depth += 1;
                    col += 1;
                    continue;
                }
                ']' => {
                    tokens.push(Token::new(TokenType::RBracket, "]", line_num, col + 1));
                    if paren_depth > 0 {
                        paren_depth -= 1;
                    }
                    col += 1;
                    continue;
                }
                _ => {}
            }

            if ch == '.' && col + 1 < line.len() && line[col + 1].is_ascii_alphabetic() {
                tokens.push(Token::new(TokenType::Dot, ".", line_num, col + 1));
                col += 1;
                continue;
            }

            if ch == '-' && col + 1 < line.len() && line[col + 1] != '>' {
                tokens.push(Token::new(TokenType::Minus, "-", line_num, col + 1));
                col += 1;
                continue;
            }

            if block && ch == ':' {
                tokens.push(Token::new(TokenType::Colon, ":", line_num, col + 1));
                col += 1;
                continue;
            }
            if block && ch == '%' {
                tokens.push(Token::new(TokenType::Percent, "%", line_num, col + 1));
                col += 1;
                continue;
            }

            if ch == '{' {
                let mut scan = col + 1;
                while scan < line.len()
                    && (line[scan].is_ascii_alphanumeric() || line[scan] == '_')
                {
                    scan += 1;
                }
                if block && scan > col + 1 && scan < line.len() && line[scan] == '}' {
                    let name: String = line[col + 1..scan].iter().collect();
                    tokens.push(Token::new(TokenType::Placeholder, name, line_num, col + 1));
                    col = scan + 1;
                    col = try_emit_time_unit(line, col, line_num, &mut tokens);
                    continue;
                }
                tokens.push(Token::new(TokenType::LBrace, "{", line_num, col + 1));
                paren_depth += 1;
                col += 1;
                continue;
            }
            if ch == '}' {
                tokens.push(Token::new(TokenType::RBrace, "}", line_num, col + 1));
                if paren_depth > 0 {
                    paren_depth -= 1;
                }
                col += 1;
                continue;
            }

            if ch == '"' {
                let start_col = col;
                col += 1;
                let mut buf = String::new();
                while col < line.len() && line[col] != '"' {
                    buf.push(line[col]);
                    col += 1;
                }
                if col >= line.len() {
                    return Err(DslError::lex(
                        "UNTERMINATED_STRING: unterminated string literal",
                        SourceSpan::new(line_num, start_col + 1),
                    ));
                }
                col += 1;
                tokens.push(Token::new(
                    TokenType::StringLiteral,
                    buf,
                    line_num,
                    start_col + 1,
                ));
                continue;
            }

            if ch.is_ascii_digit()
                || (ch == '.' && col + 1 < line.len() && line[col + 1].is_ascii_digit())
            {
                if block
                    && ch.is_ascii_digit()
                    && matches!(
                        tokens.last().map(|t| t.ty),
                        Some(TokenType::Phase) | Some(TokenType::Experiment)
                    )
                {
                    let start_col = col;
                    let mut ident = String::new();
                    while col < line.len()
                        && (line[col].is_ascii_alphanumeric() || line[col] == '_')
                    {
                        ident.push(line[col]);
                        col += 1;
                    }
                    tokens.push(Token::new(
                        TokenType::Identifier,
                        ident,
                        line_num,
                        start_col + 1,
                    ));
                    continue;
                }
                let start_col = col;
                let mut num = String::new();
                while col < line.len() && (line[col].is_ascii_digit() || line[col] == '.') {
                    num.push(line[col]);
                    col += 1;
                }
                tokens.push(Token::new(TokenType::Number, num, line_num, start_col + 1));
                col = try_emit_time_unit(line, col, line_num, &mut tokens);
                continue;
            }

            if ch.is_ascii_alphabetic() || ch == '_' {
                let start_col = col;
                let mut word = String::new();
                while col < line.len()
                    && (line[col].is_ascii_alphanumeric() || line[col] == '_')
                {
                    word.push(line[col]);
                    col += 1;
                }

                if let Some(tok) = classify_word(&word, line_num, start_col + 1, block) {
                    tokens.push(tok.clone());
                    match tok.ty {
                        TokenType::ScheduleType => {
                            let keyword = &word[..2];
                            let rest = &word[2..];
                            if !rest.is_empty() {
                                *tokens.last_mut().unwrap() = Token::new(
                                    TokenType::ScheduleType,
                                    keyword,
                                    line_num,
                                    start_col + 1,
                                );
                                let mut decimal_extra = String::new();
                                if col < line.len() && line[col] == '.' {
                                    let mut dc = col + 1;
                                    while dc < line.len() && line[dc].is_ascii_digit() {
                                        dc += 1;
                                    }
                                    if dc > col + 1 {
                                        decimal_extra =
                                            line[col..dc].iter().collect::<String>();
                                        col = dc;
                                    }
                                }
                                let mut rest_full = rest.to_string();
                                rest_full.push_str(&decimal_extra);
                                lex_number_suffix(
                                    &rest_full,
                                    &mut tokens,
                                    line_num,
                                    start_col + 3,
                                )?;
                            }
                        }
                        TokenType::DrMod => {
                            let keyword = &word[..3];
                            let rest = &word[3..];
                            if !rest.is_empty() {
                                *tokens.last_mut().unwrap() = Token::new(
                                    TokenType::DrMod,
                                    keyword,
                                    line_num,
                                    start_col + 1,
                                );
                                lex_number_suffix(rest, &mut tokens, line_num, start_col + 4)?;
                            }
                        }
                        TokenType::Lh => {
                            let prefix_len = if word.starts_with("LimitedHold") { 11 } else { 2 };
                            if word.len() > prefix_len {
                                let keyword = &word[..prefix_len];
                                let rest = &word[prefix_len..];
                                *tokens.last_mut().unwrap() = Token::new(
                                    TokenType::Lh,
                                    keyword,
                                    line_num,
                                    start_col + 1,
                                );
                                lex_number_suffix(
                                    rest,
                                    &mut tokens,
                                    line_num,
                                    start_col + prefix_len + 1,
                                )?;
                            }
                        }
                        TokenType::Pr if word.len() > 2 => {
                            *tokens.last_mut().unwrap() =
                                Token::new(TokenType::Pr, "PR", line_num, start_col + 1);
                            lex_number_suffix(&word[2..], &mut tokens, line_num, start_col + 3)?;
                        }
                        _ => {}
                    }
                    continue;
                }
                return Err(DslError::lex(
                    format!("unexpected word: '{}'", word),
                    SourceSpan::with_length(line_num, start_col + 1, word.len()),
                ));
            }

            return Err(DslError::lex(
                format!("UNEXPECTED_CHARACTER: '{}'", ch),
                SourceSpan::new(line_num, col + 1),
            ));
        }

        if block && paren_depth == 0 && tokens.len() > start_token_count {
            tokens.push(Token::new(
                TokenType::Newline,
                "\\n",
                line_num,
                line.len() + 1,
            ));
        }
    }

    if let Some(last) = tokens.last() {
        if last.ty == TokenType::Newline {
            tokens.pop();
        }
    }

    let last_line_len = lines.last().map(|l| l.len()).unwrap_or(0);
    let eof_line = lines.len().max(1);
    tokens.push(Token::new(TokenType::Eof, "", eof_line, last_line_len + 1));
    Ok(tokens)
}

fn try_emit_time_unit(
    line: &[char],
    col: usize,
    line_num: usize,
    tokens: &mut Vec<Token>,
) -> usize {
    let mut probe = col;
    if probe < line.len() && line[probe] == '-' {
        if probe + 1 < line.len() && line[probe + 1] == '>' {
            return col;
        }
        probe += 1;
    } else {
        while probe < line.len() && (line[probe] == ' ' || line[probe] == '\t') {
            probe += 1;
        }
    }
    let remaining: String = line[probe..].iter().collect();

    if remaining.starts_with("min") {
        let end = probe + 3;
        let ok = end == line.len()
            || !(line[end].is_ascii_alphanumeric() || line[end] == '_');
        if ok {
            tokens.push(Token::new(TokenType::TimeUnit, "min", line_num, probe + 1));
            return end;
        }
    }
    if remaining.starts_with("sec") {
        let end = probe + 3;
        let ok = end == line.len()
            || !(line[end].is_ascii_alphanumeric() || line[end] == '_');
        if ok {
            tokens.push(Token::new(TokenType::TimeUnit, "s", line_num, probe + 1));
            return end;
        }
    }
    if remaining.starts_with("ms") {
        let end = probe + 2;
        let ok = end == line.len() || !line[end].is_ascii_alphabetic();
        if ok {
            tokens.push(Token::new(TokenType::TimeUnit, "ms", line_num, probe + 1));
            return end;
        }
    }
    if remaining.starts_with('s')
        && (probe + 1 >= line.len() || !line[probe + 1].is_ascii_alphabetic())
    {
        tokens.push(Token::new(TokenType::TimeUnit, "s", line_num, probe + 1));
        return probe + 1;
    }
    col
}

fn lex_number_suffix(
    rest: &str,
    tokens: &mut Vec<Token>,
    line: usize,
    mut col: usize,
) -> Result<()> {
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
        i += 1;
    }
    let num_part: String = chars[..i].iter().collect();
    let unit_part: String = chars[i..].iter().collect();

    if !num_part.is_empty() {
        let len = num_part.len();
        tokens.push(Token::new(TokenType::Number, num_part, line, col));
        col += len;
    }

    if !unit_part.is_empty() {
        let canonical = match unit_part.as_str() {
            "sec" => Some("s"),
            "s" | "ms" | "min" => Some(unit_part.as_str()),
            _ => None,
        };
        match canonical {
            Some(v) => {
                tokens.push(Token::new(TokenType::TimeUnit, v, line, col));
            }
            None => {
                return Err(DslError::lex(
                    format!(
                        "unrecognized time unit suffix: '{}' (expected 's', 'ms', or 'min')",
                        unit_part
                    ),
                    SourceSpan::with_length(line, col, unit_part.len()),
                ));
            }
        }
    }
    Ok(())
}

fn classify_word(word: &str, line: usize, col: usize, block: bool) -> Option<Token> {
    if block {
        if let Some(ty) = experiment_keyword(word) {
            return Some(Token::new(ty, word, line, col));
        }
    }

    if is_kw_arg_keyword(word) {
        return Some(Token::new(TokenType::KwArgName, word, line, col));
    }

    if word.len() >= 2 && is_schedule_type_keyword(&word[..2]) {
        let after_prefix = word.chars().nth(2);
        if word.len() == 2 || after_prefix.is_some_and(|c| c.is_ascii_digit()) {
            return Some(Token::new(TokenType::ScheduleType, word, line, col));
        }
    }

    if word.len() >= 3 && DR_KEYWORDS.contains(&&word[..3]) {
        let after_prefix = word.chars().nth(3);
        if word.len() == 3 || after_prefix.is_some_and(|c| c.is_ascii_digit()) {
            return Some(Token::new(TokenType::DrMod, word, line, col));
        }
    }

    if SPECIAL_KEYWORDS.contains(&word) {
        let ty = if word == "EXT" { TokenType::Ext } else { TokenType::Crf };
        return Some(Token::new(ty, word, line, col));
    }
    if COMBINATOR_KEYWORDS.contains(&word) {
        return Some(Token::new(TokenType::Combinator, word, line, col));
    }
    if word.len() >= 2 && &word[..2] == "LH" {
        let after = word.chars().nth(2);
        if word.len() == 2 || after.is_some_and(|c| c.is_ascii_digit() || c == '.') {
            return Some(Token::new(TokenType::Lh, word, line, col));
        }
    }
    if word.len() >= 11 && &word[..11] == "LimitedHold" {
        let after = word.chars().nth(11);
        if word.len() == 11 || after.is_some_and(|c| c.is_ascii_digit() || c == '.') {
            return Some(Token::new(TokenType::Lh, word, line, col));
        }
    }
    if word == "RD" || word == "ReinforcementDelay" {
        return Some(Token::new(TokenType::Rd, word, line, col));
    }
    if word == "PR" {
        return Some(Token::new(TokenType::Pr, word, line, col));
    }
    if word.len() >= 3 && &word[..2] == "PR" {
        if let Some(c) = word.chars().nth(2) {
            if c.is_ascii_digit() {
                return Some(Token::new(TokenType::Pr, word, line, col));
            }
        }
    }
    if word == "Repeat" {
        return Some(Token::new(TokenType::Repeat, word, line, col));
    }
    if word == "let" {
        return Some(Token::new(TokenType::Let, word, line, col));
    }
    if DISTRIBUTION_NAMES.contains(&word) {
        return Some(Token::new(TokenType::DistributionName, word, line, col));
    }
    if PR_PARAMS.contains(&word) {
        return Some(Token::new(TokenType::PrParam, word, line, col));
    }

    let first = word.chars().next()?;
    if first.is_ascii_lowercase() || first == '_' {
        // Reserved lowercase words still emit as identifiers here; the parser
        // rejects misuse.
        let _ = LOWERCASE_RESERVED;
        return Some(Token::new(TokenType::Identifier, word, line, col));
    }

    Some(Token::new(TokenType::Identifier, word, line, col))
}

fn experiment_keyword(word: &str) -> Option<TokenType> {
    Some(match word {
        "phase" => TokenType::Phase,
        "sessions" => TokenType::Sessions,
        "stable" => TokenType::Stable,
        "use" => TokenType::Use,
        "no_schedule" => TokenType::NoSchedule,
        "shaping" => TokenType::Shaping,
        "progressive" => TokenType::Progressive,
        "steps" => TokenType::Steps,
        "interleave" => TokenType::Interleave,
        "no_trailing" => TokenType::NoTrailing,
        "advance" => TokenType::Advance,
        "when" => TokenType::When,
        "experiment" => TokenType::Experiment,
        _ => return None,
    })
}
