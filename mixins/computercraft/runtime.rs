// CC: Tweaked for Horizon — the terminal and the Lua subset a computer runs.
//
// CC: Tweaked's computer is a Lua 5.2 machine (`bit32`, `loadstring` and
// coroutines in `data/computercraft/lua/bios.lua`) running a ROM of 104 files.
// This pin has **no Lua crate in its lock**, and a bundle crate may only use
// packages the lock already has (`E_CRATE_DEPENDENCY`, measured), so the port
// implements a documented **subset** of Lua 5.2 instead of pretending to run
// the ROM. `notes.md` carries the exact gap list.
//
// Pure: no world access, no I/O. The terminal is a value the glue owns.
use std::collections::BTreeMap;

/// The biggest number of statements and expressions one program may evaluate
/// before the computer stops it. CC kills a program that never yields with
/// "Too long without yielding"; the subset does not yield at all, so the guard
/// is a step count instead of a wall-clock timer (documented deviation).
pub const RUN_STEP_LIMIT: u32 = 100_000;

/// The terminal the block screen shows: 16 columns of 6-px glyphs across the
/// face and 10 rows of 9-px lines (upstream's basic terminal is 51x19; a
/// one-block screen is smaller, so the stored buffer is smaller too).
pub const TERM_WIDTH: usize = 16;
pub const TERM_LINES: usize = 10;

/// The `facing` a placed computer gets: it faces the player, as
/// `AbstractComputerBlock.getStateForPlacement` does
/// (`context.getHorizontalDirection().getOpposite()`). Vanilla's
/// `Direction.fromYaw` indexes `[south, west, north, east]` by
/// `floor(yaw / 90 + 0.5) & 3`, so the opposite is `[north, east, south, west]`.
#[must_use]
pub fn facing_for_placement(yaw: f32) -> &'static str {
    const FACING: [&str; 4] = ["north", "east", "south", "west"];
    let quarter = (f64::from(yaw) / 90.0 + 0.5).floor() as i64;
    FACING[(quarter & 3) as usize]
}

/// A computer's terminal: `width` columns, `max_lines` rows, oldest scrolled
/// off. Upstream's basic terminal is 51x19; a port block has no monitor panel,
/// so the glue keeps a shorter buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terminal {
    width: usize,
    max_lines: usize,
    lines: Vec<String>,
}

impl Terminal {
    #[must_use]
    pub fn new(width: usize, max_lines: usize) -> Self {
        Self {
            width: width.max(1),
            max_lines: max_lines.max(1),
            lines: Vec::new(),
        }
    }

    /// Writes one line, wrapping at the terminal's width.
    pub fn write_line(&mut self, line: &str) {
        for source in line.split('\n') {
            let mut chunk = String::new();
            let mut columns = 0;
            for character in source.chars() {
                chunk.push(character);
                columns += 1;
                if columns == self.width {
                    self.push(chunk);
                    chunk = String::new();
                    columns = 0;
                }
            }
            if columns > 0 || source.is_empty() {
                self.push(chunk);
            }
        }
    }

    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    fn push(&mut self, line: String) {
        self.lines.push(line);
        let excess = self.lines.len().saturating_sub(self.max_lines);
        if excess > 0 {
            self.lines.drain(..excess);
        }
    }
}

/// Why a program stopped before its last statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaError {
    /// The chunk did not parse.
    Syntax { line: usize, message: String },
    /// The chunk parsed and then failed while running.
    Runtime { line: usize, message: String },
}

impl core::fmt::Display for LuaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Syntax { line, message } => write!(f, "line {line}: {message}"),
            Self::Runtime { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl LuaError {
    fn syntax(line: usize, message: impl Into<String>) -> Self {
        Self::Syntax {
            line,
            message: message.into(),
        }
    }

    fn runtime(line: usize, message: impl Into<String>) -> Self {
        Self::Runtime {
            line,
            message: message.into(),
        }
    }
}

/// Runs `program`, writing what it prints into `terminal`.
///
/// # Errors
/// A syntax error in the chunk, or a runtime error the program raised.
pub fn run(program: &str, terminal: &mut Terminal) -> Result<(), LuaError> {
    let tokens = lex(program)?;
    let chunk = Parser::new(tokens).parse_chunk()?;
    let mut machine = Machine {
        terminal,
        scopes: vec![BTreeMap::new()],
        steps: 0,
        line: 1,
    };
    machine.run(&chunk)
}

// ---------------------------------------------------------------- tokens

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Number(f64),
    Str(String),
    Keyword(Keyword),
    Symbol(Symbol),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keyword {
    And,
    Break,
    Do,
    Else,
    Elseif,
    End,
    False,
    For,
    Function,
    If,
    In,
    Local,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Then,
    True,
    Until,
    While,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Symbol {
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Hash,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Assign,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Colon,
    Comma,
    Dot,
}

struct Lexer<'a> {
    source: &'a [u8],
    position: usize,
    line: usize,
}

fn lex(source: &str) -> Result<Vec<(Token, usize)>, LuaError> {
    Lexer {
        source: source.as_bytes(),
        position: 0,
        line: 1,
    }
    .tokens()
}

impl Lexer<'_> {
    fn tokens(mut self) -> Result<Vec<(Token, usize)>, LuaError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_space_and_comments()?;
            let line = self.line;
            let Some(byte) = self.peek() else {
                tokens.push((Token::Eof, line));
                return Ok(tokens);
            };
            let token = if byte.is_ascii_alphabetic() || byte == b'_' {
                self.name()
            } else if byte.is_ascii_digit() {
                self.number()?
            } else if byte == b'"' || byte == b'\'' {
                self.string()?
            } else {
                self.symbol()?
            };
            tokens.push((token, line));
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.position).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.source.get(self.position + offset).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        if byte == b'\n' {
            self.line += 1;
        }
        Some(byte)
    }

    fn skip_space_and_comments(&mut self) -> Result<(), LuaError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => {
                    self.bump();
                }
                Some(b'-') if self.peek_at(1) == Some(b'-') => {
                    self.bump();
                    self.bump();
                    if self.peek() == Some(b'[') && self.peek_at(1) == Some(b'[') {
                        self.bump();
                        self.bump();
                        loop {
                            match self.bump() {
                                None => {
                                    return Err(LuaError::syntax(
                                        self.line,
                                        "unfinished long comment",
                                    ));
                                }
                                Some(b']') if self.peek() == Some(b']') => {
                                    self.bump();
                                    break;
                                }
                                Some(_) => {}
                            }
                        }
                    } else {
                        while let Some(byte) = self.peek() {
                            if byte == b'\n' {
                                break;
                            }
                            self.bump();
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn name(&mut self) -> Token {
        let mut word = String::new();
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                word.push(char::from(byte));
                self.bump();
            } else {
                break;
            }
        }
        let keyword = match word.as_str() {
            "and" => Some(Keyword::And),
            "break" => Some(Keyword::Break),
            "do" => Some(Keyword::Do),
            "else" => Some(Keyword::Else),
            "elseif" => Some(Keyword::Elseif),
            "end" => Some(Keyword::End),
            "false" => Some(Keyword::False),
            "for" => Some(Keyword::For),
            "function" => Some(Keyword::Function),
            "if" => Some(Keyword::If),
            "in" => Some(Keyword::In),
            "local" => Some(Keyword::Local),
            "nil" => Some(Keyword::Nil),
            "not" => Some(Keyword::Not),
            "or" => Some(Keyword::Or),
            "repeat" => Some(Keyword::Repeat),
            "return" => Some(Keyword::Return),
            "then" => Some(Keyword::Then),
            "true" => Some(Keyword::True),
            "until" => Some(Keyword::Until),
            "while" => Some(Keyword::While),
            _ => None,
        };
        keyword.map_or(Token::Name(word), Token::Keyword)
    }

    fn number(&mut self) -> Result<Token, LuaError> {
        let start = self.position;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.bump();
        }
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
            self.bump();
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.bump();
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exponent = self.position;
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.position = exponent;
            } else {
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.bump();
                }
            }
        }
        let text = core::str::from_utf8(&self.source[start..self.position])
            .unwrap_or_default()
            .to_owned();
        text.parse::<f64>()
            .map(Token::Number)
            .map_err(|_| LuaError::syntax(self.line, format!("malformed number near '{text}'")))
    }

    fn string(&mut self) -> Result<Token, LuaError> {
        let quote = self.bump();
        let mut bytes = Vec::new();
        loop {
            let Some(byte) = self.bump() else {
                return Err(LuaError::syntax(self.line, "unfinished string"));
            };
            if Some(byte) == quote {
                return String::from_utf8(bytes)
                    .map(Token::Str)
                    .map_err(|_| LuaError::syntax(self.line, "string is not UTF-8"));
            }
            if byte != b'\\' {
                bytes.push(byte);
                continue;
            }
            let Some(escape) = self.bump() else {
                return Err(LuaError::syntax(self.line, "unfinished string"));
            };
            match escape {
                b'n' => bytes.push(b'\n'),
                b't' => bytes.push(b'\t'),
                b'r' => bytes.push(b'\r'),
                b'a' => bytes.push(0x07),
                b'b' => bytes.push(0x08),
                b'f' => bytes.push(0x0c),
                b'v' => bytes.push(0x0b),
                b'\\' => bytes.push(b'\\'),
                b'"' => bytes.push(b'"'),
                b'\'' => bytes.push(b'\''),
                other => {
                    return Err(LuaError::syntax(
                        self.line,
                        format!("invalid escape '\\{}'", char::from(other)),
                    ));
                }
            }
        }
    }

    fn symbol(&mut self) -> Result<Token, LuaError> {
        let byte = self.bump().unwrap_or_default();
        let symbol = match byte {
            b'+' => Symbol::Plus,
            b'-' => Symbol::Minus,
            b'*' => Symbol::Star,
            b'/' => Symbol::Slash,
            b'%' => Symbol::Percent,
            b'^' => Symbol::Caret,
            b'#' => Symbol::Hash,
            b'(' => Symbol::LParen,
            b')' => Symbol::RParen,
            b'{' => Symbol::LBrace,
            b'}' => Symbol::RBrace,
            b'[' => Symbol::LBracket,
            b']' => Symbol::RBracket,
            b';' => Symbol::Semi,
            b':' => Symbol::Colon,
            b',' => Symbol::Comma,
            b'.' => {
                if self.peek() == Some(b'.') {
                    self.bump();
                    Symbol::Concat
                } else {
                    Symbol::Dot
                }
            }
            b'=' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    Symbol::Eq
                } else {
                    Symbol::Assign
                }
            }
            b'~' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    Symbol::Ne
                } else {
                    return Err(LuaError::syntax(self.line, "unexpected symbol near '~'"));
                }
            }
            b'<' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    Symbol::Le
                } else {
                    Symbol::Lt
                }
            }
            b'>' => {
                if self.peek() == Some(b'=') {
                    self.bump();
                    Symbol::Ge
                } else {
                    Symbol::Gt
                }
            }
            other => {
                return Err(LuaError::syntax(
                    self.line,
                    format!("unexpected symbol near '{}'", char::from(other)),
                ));
            }
        };
        Ok(Token::Symbol(symbol))
    }
}

// ---------------------------------------------------------------- syntax tree

#[derive(Debug, Clone)]
enum Stmt {
    Local {
        names: Vec<String>,
        values: Vec<Expr>,
    },
    Assign {
        name: String,
        value: Expr,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Option<Vec<Stmt>>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    For {
        name: String,
        start: Expr,
        limit: Expr,
        step: Option<Expr>,
        body: Vec<Stmt>,
    },
    Do(Vec<Stmt>),
    Return,
}

#[derive(Debug, Clone)]
enum Expr {
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
    Name(String),
    Not(Box<Expr>),
    Negate(Box<Expr>),
    Length(Box<Expr>),
    Binary {
        operator: Operator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
    Concat,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

/// Left and right binding powers, Lua 5.2's `luaP_priority`.
fn binding_power(operator: Operator) -> (u8, u8) {
    match operator {
        Operator::Or => (1, 1),
        Operator::And => (2, 2),
        Operator::Less
        | Operator::LessEqual
        | Operator::Greater
        | Operator::GreaterEqual
        | Operator::Equal
        | Operator::NotEqual => (3, 3),
        Operator::Concat => (9, 8),
        Operator::Add | Operator::Subtract => (10, 10),
        Operator::Multiply | Operator::Divide | Operator::Modulo => (11, 11),
        Operator::Power => (14, 13),
    }
}

/// The binding power of a prefix operator (`not`, `-`, `#`).
const UNARY_PRIORITY: u8 = 12;

struct Parser {
    tokens: Vec<(Token, usize)>,
    position: usize,
}

impl Parser {
    fn new(tokens: Vec<(Token, usize)>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn parse_chunk(mut self) -> Result<Vec<Stmt>, LuaError> {
        let chunk = self.block()?;
        if !matches!(self.peek(), Token::Eof) {
            return Err(self.error(&format!("expected end of chunk, found {}", self.describe())));
        }
        Ok(chunk)
    }

    fn peek(&self) -> &Token {
        self.tokens
            .get(self.position)
            .map_or(&Token::Eof, |(token, _)| token)
    }

    fn line(&self) -> usize {
        self.tokens.get(self.position).map_or(1, |(_, line)| *line)
    }

    fn describe(&self) -> String {
        match self.peek() {
            Token::Name(name) => format!("'{name}'"),
            Token::Number(number) => format!("'{number}'"),
            Token::Str(text) => format!("string '{text}'"),
            Token::Keyword(keyword) => format!("'{keyword:?}'"),
            Token::Symbol(symbol) => format!("'{symbol:?}'"),
            Token::Eof => "end of chunk".to_owned(),
        }
    }

    fn error(&self, message: &str) -> LuaError {
        LuaError::syntax(self.line(), message.to_owned())
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if !matches!(token, Token::Eof) {
            self.position += 1;
        }
        token
    }

    fn check(&self, symbol: Symbol) -> bool {
        matches!(self.peek(), Token::Symbol(value) if *value == symbol)
    }

    fn eat(&mut self, symbol: Symbol) -> bool {
        if self.check(symbol) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, symbol: Symbol) -> Result<(), LuaError> {
        if self.eat(symbol) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {symbol:?}, found {}", self.describe())))
        }
    }

    fn check_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.peek(), Token::Keyword(value) if *value == keyword)
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        if self.check_keyword(keyword) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<(), LuaError> {
        if self.eat_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {keyword:?}, found {}", self.describe())))
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, LuaError> {
        let mut statements = Vec::new();
        loop {
            if matches!(self.peek(), Token::Eof)
                || self.check_keyword(Keyword::End)
                || self.check_keyword(Keyword::Else)
                || self.check_keyword(Keyword::Elseif)
                || self.check_keyword(Keyword::Until)
            {
                return Ok(statements);
            }
            if self.eat(Symbol::Semi) {
                continue;
            }
            statements.push(self.statement()?);
            while self.eat(Symbol::Semi) {}
        }
    }

    fn statement(&mut self) -> Result<Stmt, LuaError> {
        if self.eat_keyword(Keyword::Local) {
            let mut names = vec![self.name()?];
            while self.eat(Symbol::Comma) {
                names.push(self.name()?);
            }
            let mut values = Vec::new();
            if self.eat(Symbol::Assign) {
                values.push(self.expression()?);
                while self.eat(Symbol::Comma) {
                    values.push(self.expression()?);
                }
            }
            return Ok(Stmt::Local { names, values });
        }
        if self.eat_keyword(Keyword::If) {
            return self.if_statement();
        }
        if self.eat_keyword(Keyword::While) {
            let condition = self.expression()?;
            self.expect_keyword(Keyword::Do)?;
            let body = self.block()?;
            self.expect_keyword(Keyword::End)?;
            return Ok(Stmt::While { condition, body });
        }
        if self.eat_keyword(Keyword::For) {
            let name = self.name()?;
            self.expect(Symbol::Assign)?;
            let start = self.expression()?;
            self.expect(Symbol::Comma)?;
            let limit = self.expression()?;
            let step = if self.eat(Symbol::Comma) {
                Some(self.expression()?)
            } else {
                None
            };
            self.expect_keyword(Keyword::Do)?;
            let body = self.block()?;
            self.expect_keyword(Keyword::End)?;
            return Ok(Stmt::For {
                name,
                start,
                limit,
                step,
                body,
            });
        }
        if self.eat_keyword(Keyword::Do) {
            let body = self.block()?;
            self.expect_keyword(Keyword::End)?;
            return Ok(Stmt::Do(body));
        }
        if self.eat_keyword(Keyword::Return) {
            if !matches!(self.peek(), Token::Eof)
                && !self.check_keyword(Keyword::End)
                && !self.check_keyword(Keyword::Else)
                && !self.check_keyword(Keyword::Elseif)
                && !self.check_keyword(Keyword::Until)
                && !self.check(Symbol::Semi)
            {
                return Err(self.error("this port's Lua subset returns no values"));
            }
            return Ok(Stmt::Return);
        }
        if self.check_keyword(Keyword::Break)
            || self.check_keyword(Keyword::Repeat)
            || self.check_keyword(Keyword::Function)
            || self.check_keyword(Keyword::Until)
        {
            return Err(self.error("break/repeat/function are not in this port's Lua subset"));
        }
        let name = self.name()?;
        if self.eat(Symbol::Assign) {
            let value = self.expression()?;
            return Ok(Stmt::Assign { name, value });
        }
        self.expect(Symbol::LParen)?;
        let mut args = Vec::new();
        if !self.check(Symbol::RParen) {
            args.push(self.expression()?);
            while self.eat(Symbol::Comma) {
                args.push(self.expression()?);
            }
        }
        self.expect(Symbol::RParen)?;
        Ok(Stmt::Call { name, args })
    }

    fn if_statement(&mut self) -> Result<Stmt, LuaError> {
        let mut branches = Vec::new();
        let condition = self.expression()?;
        self.expect_keyword(Keyword::Then)?;
        branches.push((condition, self.block()?));
        while self.eat_keyword(Keyword::Elseif) {
            let condition = self.expression()?;
            self.expect_keyword(Keyword::Then)?;
            branches.push((condition, self.block()?));
        }
        let otherwise = if self.eat_keyword(Keyword::Else) {
            Some(self.block()?)
        } else {
            None
        };
        self.expect_keyword(Keyword::End)?;
        Ok(Stmt::If {
            branches,
            otherwise,
        })
    }

    fn name(&mut self) -> Result<String, LuaError> {
        match self.peek().clone() {
            Token::Name(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(self.error(&format!("expected a name, found {}", self.describe()))),
        }
    }

    fn expression(&mut self) -> Result<Expr, LuaError> {
        self.sub_expression(0)
    }

    /// Lua's precedence climbing: `limit` is the right binding power the
    /// operator must beat to be taken by this call.
    fn sub_expression(&mut self, limit: u8) -> Result<Expr, LuaError> {
        let mut left = if self.eat_keyword(Keyword::Not) {
            Expr::Not(Box::new(self.sub_expression(UNARY_PRIORITY)?))
        } else if self.eat(Symbol::Minus) {
            Expr::Negate(Box::new(self.sub_expression(UNARY_PRIORITY)?))
        } else if self.eat(Symbol::Hash) {
            Expr::Length(Box::new(self.sub_expression(UNARY_PRIORITY)?))
        } else {
            self.simple_expression()?
        };
        while let Some(operator) = self.binary_operator() {
            let (left_power, right_power) = binding_power(operator);
            if left_power <= limit {
                break;
            }
            self.advance();
            let right = self.sub_expression(right_power)?;
            left = Expr::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn binary_operator(&self) -> Option<Operator> {
        match self.peek() {
            Token::Keyword(Keyword::And) => Some(Operator::And),
            Token::Keyword(Keyword::Or) => Some(Operator::Or),
            Token::Symbol(symbol) => match symbol {
                Symbol::Plus => Some(Operator::Add),
                Symbol::Minus => Some(Operator::Subtract),
                Symbol::Star => Some(Operator::Multiply),
                Symbol::Slash => Some(Operator::Divide),
                Symbol::Percent => Some(Operator::Modulo),
                Symbol::Caret => Some(Operator::Power),
                Symbol::Concat => Some(Operator::Concat),
                Symbol::Eq => Some(Operator::Equal),
                Symbol::Ne => Some(Operator::NotEqual),
                Symbol::Lt => Some(Operator::Less),
                Symbol::Le => Some(Operator::LessEqual),
                Symbol::Gt => Some(Operator::Greater),
                Symbol::Ge => Some(Operator::GreaterEqual),
                _ => None,
            },
            _ => None,
        }
    }

    fn simple_expression(&mut self) -> Result<Expr, LuaError> {
        match self.peek().clone() {
            Token::Keyword(Keyword::Nil) => {
                self.advance();
                Ok(Expr::Nil)
            }
            Token::Keyword(Keyword::True) => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::Keyword(Keyword::False) => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Number(number) => {
                self.advance();
                Ok(Expr::Number(number))
            }
            Token::Str(text) => {
                self.advance();
                Ok(Expr::Str(text))
            }
            Token::Name(name) => {
                self.advance();
                Ok(Expr::Name(name))
            }
            Token::Symbol(Symbol::LParen) => {
                self.advance();
                let inner = self.expression()?;
                self.expect(Symbol::RParen)?;
                Ok(inner)
            }
            _ => Err(self.error(&format!("unexpected symbol near {}", self.describe()))),
        }
    }
}

// ---------------------------------------------------------------- values

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
}

impl Value {
    fn truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    /// Lua's name for a value in an error message.
    fn type_name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::Str(_) => "string",
        }
    }

    /// Lua's `tostring` for the values the subset has.
    fn to_text(&self) -> String {
        match self {
            Self::Nil => "nil".to_owned(),
            Self::Bool(value) => value.to_string(),
            Self::Number(number) => format_number(*number),
            Self::Str(text) => text.clone(),
        }
    }
}

/// Lua prints a number with `%.14g`: a whole number has no decimal point.
fn format_number(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    format!("{value}")
}

struct Machine<'a> {
    terminal: &'a mut Terminal,
    scopes: Vec<BTreeMap<String, Value>>,
    steps: u32,
    line: usize,
}

impl Machine<'_> {
    fn tick(&mut self) -> Result<(), LuaError> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > RUN_STEP_LIMIT {
            return Err(LuaError::runtime(
                self.line,
                format!("too long running (the subset stops after {RUN_STEP_LIMIT} steps)"),
            ));
        }
        Ok(())
    }

    fn run(&mut self, chunk: &[Stmt]) -> Result<(), LuaError> {
        self.exec_block(chunk).map(|_returned| ())
    }

    /// Runs statements in the current scope; `true` when one of them ended the
    /// chunk (`return`).
    fn exec_block(&mut self, chunk: &[Stmt]) -> Result<bool, LuaError> {
        for statement in chunk {
            if self.execute(statement)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Runs one statement; `true` when it ended the chunk (`return`).
    fn execute(&mut self, statement: &Stmt) -> Result<bool, LuaError> {
        self.tick()?;
        match statement {
            Stmt::Local { names, values } => {
                for (index, name) in names.iter().enumerate() {
                    let value = match values.get(index) {
                        Some(expression) => self.evaluate(expression)?,
                        None => Value::Nil,
                    };
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(name.clone(), value);
                    }
                }
            }
            Stmt::Assign { name, value } => {
                let value = self.evaluate(value)?;
                self.assign(name, value);
            }
            Stmt::Call { name, args } => {
                let mut values = Vec::with_capacity(args.len());
                for argument in args {
                    values.push(self.evaluate(argument)?);
                }
                self.call(name, &values)?;
            }
            Stmt::If {
                branches,
                otherwise,
            } => {
                for (condition, body) in branches {
                    if self.evaluate(condition)?.truthy() {
                        return self.run_body(body);
                    }
                }
                if let Some(body) = otherwise {
                    return self.run_body(body);
                }
            }
            Stmt::While { condition, body } => {
                while self.evaluate(condition)?.truthy() {
                    if self.run_body(body)? {
                        return Ok(true);
                    }
                }
            }
            Stmt::For {
                name,
                start,
                limit,
                step,
                body,
            } => {
                let start_value = self.evaluate(start)?;
                let start = self.number(start_value)?;
                let limit_value = self.evaluate(limit)?;
                let limit = self.number(limit_value)?;
                let step = match step {
                    Some(expression) => {
                        let value = self.evaluate(expression)?;
                        self.number(value)?
                    }
                    None => 1.0,
                };
                if step == 0.0 {
                    return Err(LuaError::runtime(self.line, "'for' step is zero"));
                }
                self.scopes.push(BTreeMap::new());
                let mut value = start;
                while (step > 0.0 && value <= limit) || (step < 0.0 && value >= limit) {
                    self.tick()?;
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(name.clone(), Value::Number(value));
                    }
                    if self.run_body(body)? {
                        self.scopes.pop();
                        return Ok(true);
                    }
                    value += step;
                }
                self.scopes.pop();
            }
            Stmt::Do(body) => return self.run_body(body),
            Stmt::Return => return Ok(true),
        }
        Ok(false)
    }

    /// Runs a nested block in its own scope; `true` when it ended with `return`.
    fn run_body(&mut self, body: &[Stmt]) -> Result<bool, LuaError> {
        self.scopes.push(BTreeMap::new());
        let result = self.exec_block(body);
        self.scopes.pop();
        result
    }

    fn assign(&mut self, name: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(slot) = scope.get_mut(name) {
                *slot = value;
                return;
            }
        }
        if let Some(globals) = self.scopes.first_mut() {
            globals.insert(name.to_owned(), value);
        }
    }

    fn lookup(&self, name: &str) -> Value {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return value.clone();
            }
        }
        Value::Nil
    }

    fn call(&mut self, name: &str, arguments: &[Value]) -> Result<(), LuaError> {
        if name == "print" {
            let line = arguments
                .iter()
                .map(Value::to_text)
                .collect::<Vec<_>>()
                .join("\t");
            self.terminal.write_line(&line);
            return Ok(());
        }
        let callee = self.lookup(name);
        Err(LuaError::runtime(
            self.line,
            format!(
                "attempt to call a {} value (global '{name}')",
                callee.type_name()
            ),
        ))
    }

    fn number(&self, value: Value) -> Result<f64, LuaError> {
        match value {
            Value::Number(number) => Ok(number),
            other => Err(LuaError::runtime(
                self.line,
                format!(
                    "attempt to perform arithmetic on a {} value",
                    other.type_name()
                ),
            )),
        }
    }

    fn evaluate(&mut self, expression: &Expr) -> Result<Value, LuaError> {
        self.tick()?;
        match expression {
            Expr::Nil => Ok(Value::Nil),
            Expr::Bool(value) => Ok(Value::Bool(*value)),
            Expr::Number(number) => Ok(Value::Number(*number)),
            Expr::Str(text) => Ok(Value::Str(text.clone())),
            Expr::Name(name) => Ok(self.lookup(name)),
            Expr::Not(inner) => Ok(Value::Bool(!self.evaluate(inner)?.truthy())),
            Expr::Negate(inner) => {
                let value = self.evaluate(inner)?;
                Ok(Value::Number(-self.number(value)?))
            }
            Expr::Length(inner) => match self.evaluate(inner)? {
                Value::Str(text) => Ok(Value::Number(text.len() as f64)),
                other => Err(LuaError::runtime(
                    self.line,
                    format!("attempt to get length of a {} value", other.type_name()),
                )),
            },
            Expr::Binary {
                operator,
                left,
                right,
            } => self.binary(*operator, left, right),
        }
    }

    fn binary(&mut self, operator: Operator, left: &Expr, right: &Expr) -> Result<Value, LuaError> {
        if operator == Operator::And {
            let value = self.evaluate(left)?;
            return if value.truthy() {
                self.evaluate(right)
            } else {
                Ok(value)
            };
        }
        if operator == Operator::Or {
            let value = self.evaluate(left)?;
            return if value.truthy() {
                Ok(value)
            } else {
                self.evaluate(right)
            };
        }
        let left = self.evaluate(left)?;
        let right = self.evaluate(right)?;
        match operator {
            Operator::Add => Ok(Value::Number(self.number(left)? + self.number(right)?)),
            Operator::Subtract => Ok(Value::Number(self.number(left)? - self.number(right)?)),
            Operator::Multiply => Ok(Value::Number(self.number(left)? * self.number(right)?)),
            Operator::Divide => Ok(Value::Number(self.number(left)? / self.number(right)?)),
            Operator::Modulo => Ok(Value::Number(self.number(left)? % self.number(right)?)),
            Operator::Power => Ok(Value::Number(self.number(left)?.powf(self.number(right)?))),
            Operator::Concat => {
                let text = |value: &Value| match value {
                    Value::Str(text) => Some(text.clone()),
                    Value::Number(number) => Some(format_number(*number)),
                    _ => None,
                };
                match (text(&left), text(&right)) {
                    (Some(left), Some(right)) => Ok(Value::Str(left + &right)),
                    _ => {
                        let bad = if text(&left).is_none() { &left } else { &right };
                        Err(LuaError::runtime(
                            self.line,
                            format!("attempt to concatenate a {} value", bad.type_name()),
                        ))
                    }
                }
            }
            Operator::Equal => Ok(Value::Bool(equal(&left, &right))),
            Operator::NotEqual => Ok(Value::Bool(!equal(&left, &right))),
            Operator::Less | Operator::LessEqual | Operator::Greater | Operator::GreaterEqual => {
                let ordering = compare(&left, &right).ok_or_else(|| {
                    LuaError::runtime(
                        self.line,
                        format!(
                            "attempt to compare {} with {}",
                            left.type_name(),
                            right.type_name()
                        ),
                    )
                })?;
                let result = match operator {
                    Operator::Less => ordering.is_lt(),
                    Operator::LessEqual => ordering.is_le(),
                    Operator::Greater => ordering.is_gt(),
                    _ => ordering.is_ge(),
                };
                Ok(Value::Bool(result))
            }
            Operator::And | Operator::Or => unreachable!("handled above"),
        }
    }
}

fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => left == right,
        (Value::Str(left), Value::Str(right)) => left == right,
        _ => false,
    }
}

fn compare(left: &Value, right: &Value) -> Option<core::cmp::Ordering> {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.partial_cmp(right),
        (Value::Str(left), Value::Str(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a program in a wide terminal and returns its lines.
    fn out(program: &str) -> Vec<String> {
        let mut terminal = Terminal::new(51, 19);
        let result = run(program, &mut terminal);
        assert_eq!(result, Ok(()), "program failed: {program}");
        terminal.lines().to_vec()
    }

    fn err(program: &str) -> LuaError {
        let mut terminal = Terminal::new(51, 19);
        run(program, &mut terminal).expect_err("expected an error")
    }

    #[test]
    fn print_adds_numbers() {
        assert_eq!(out("print(1+1)"), ["2"]);
    }

    #[test]
    fn print_is_tab_separated() {
        assert_eq!(out(r#"print(1, "a", true)"#), ["1\ta\ttrue"]);
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_eq!(out("print(1 + 2 * 3)"), ["7"]);
        assert_eq!(out("print((1 + 2) * 3)"), ["9"]);
    }

    #[test]
    fn power_is_right_associative() {
        assert_eq!(out("print(2 ^ 3 ^ 2)"), ["512"]);
    }

    #[test]
    fn concatenation_binds_looser_than_addition() {
        assert_eq!(out(r#"print("n=" .. 1 + 1)"#), ["n=2"]);
    }

    #[test]
    fn concatenation_turns_numbers_into_text() {
        assert_eq!(out("print(1 .. 2)"), ["12"]);
    }

    #[test]
    fn unary_minus_and_length() {
        assert_eq!(out(r#"print(-3, #"abcd")"#), ["-3\t4"]);
    }

    #[test]
    fn whole_numbers_print_without_a_decimal_point() {
        assert_eq!(out("print(10 / 2, 7 / 2)"), ["5\t3.5"]);
    }

    #[test]
    fn comparisons_and_short_circuit_operators() {
        assert_eq!(
            out(r#"print(1 < 2, 2 ~= 2, true and false, nil or "x")"#),
            ["true\tfalse\tfalse\tx"]
        );
    }

    #[test]
    fn not_always_gives_a_boolean() {
        assert_eq!(out("print(not nil, not 1)"), ["true\tfalse"]);
    }

    #[test]
    fn string_escapes() {
        assert_eq!(out(r#"print("a\tb\nc\\d\"e")"#), ["a\tb", "c\\d\"e"]);
    }

    #[test]
    fn line_and_block_comments_are_skipped() {
        assert_eq!(out("-- hello\nprint(1) --[[ block ]]"), ["1"]);
    }

    #[test]
    fn locals_and_assignment() {
        assert_eq!(out("local x = 5\nx = x + 1\nprint(x)"), ["6"]);
    }

    #[test]
    fn if_elseif_else_picks_one_branch() {
        let program = r#"
            local n = 2
            if n == 1 then print("one")
            elseif n == 2 then print("two")
            else print("many") end
        "#;
        assert_eq!(out(program), ["two"]);
    }

    #[test]
    fn while_loop_runs_until_its_condition_fails() {
        let program = "local n = 0\nwhile n < 3 do n = n + 1 print(n) end";
        assert_eq!(out(program), ["1", "2", "3"]);
    }

    #[test]
    fn numeric_for_counts_up() {
        assert_eq!(out("for i = 1, 3 do print(i) end"), ["1", "2", "3"]);
    }

    #[test]
    fn numeric_for_honours_its_step() {
        assert_eq!(out("for i = 3, 1, -1 do print(i) end"), ["3", "2", "1"]);
    }

    #[test]
    fn blocks_nest() {
        let program = r#"
            for i = 1, 2 do
                if i == 2 then
                    local n = 0
                    while n < 2 do n = n + 1 print(i .. n) end
                end
            end
        "#;
        assert_eq!(out(program), ["21", "22"]);
    }

    #[test]
    fn semicolons_split_statements() {
        assert_eq!(out("print(1); print(2)"), ["1", "2"]);
    }

    #[test]
    fn nil_prints_as_nil() {
        assert_eq!(out("print(nil)"), ["nil"]);
    }

    #[test]
    fn length_counts_bytes_like_lua() {
        // "héllo" is 6 bytes in UTF-8; Lua's # counts bytes.
        assert_eq!(out(r#"print(#"héllo")"#), ["6"]);
    }

    #[test]
    fn calling_a_nil_global_is_a_runtime_error() {
        let error = err("foo()");
        assert!(
            matches!(&error, LuaError::Runtime { message, .. } if message.contains("nil value")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_syntax_error_names_its_line() {
        let error = err("print(1)\nprint(");
        assert!(
            matches!(&error, LuaError::Syntax { line: 2, message } if message.contains("expected")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn an_endless_loop_hits_the_step_budget() {
        let error = err("while true do end");
        assert!(
            matches!(&error, LuaError::Runtime { message, .. } if message.contains("too long running")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn terminal_wraps_at_its_width() {
        let mut terminal = Terminal::new(5, 4);
        terminal.write_line("abcdefg");
        assert_eq!(terminal.lines(), ["abcde", "fg"]);
    }

    #[test]
    fn terminal_keeps_the_last_lines() {
        let mut terminal = Terminal::new(51, 3);
        for n in 1..=6 {
            terminal.write_line(&n.to_string());
        }
        assert_eq!(terminal.lines(), ["4", "5", "6"]);
    }

    #[test]
    fn strings_compare_lexicographically() {
        assert_eq!(out(r#"print("a" < "b", "a" == "a")"#), ["true\ttrue"]);
    }

    #[test]
    fn comparing_a_number_with_a_string_is_an_error() {
        let error = err(r#"print(1 < "a")"#);
        assert!(
            matches!(&error, LuaError::Runtime { message, .. } if message.contains("attempt to compare")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn concatenating_a_boolean_is_an_error() {
        let error = err(r#"print("a" .. true)"#);
        assert!(
            matches!(&error, LuaError::Runtime { message, .. } if message.contains("attempt to concatenate")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_placed_computer_faces_the_player() {
        // Vanilla `Direction.fromYaw` is [south, west, north, east]; the
        // computer takes its opposite, so yaw 0 (looking south) gives north.
        assert_eq!(facing_for_placement(0.0), "north");
        assert_eq!(facing_for_placement(90.0), "east");
        assert_eq!(facing_for_placement(180.0), "south");
        assert_eq!(facing_for_placement(-90.0), "west");
        // The same quadrant at 44 degrees away is still the same direction.
        assert_eq!(facing_for_placement(44.0), "north");
        assert_eq!(facing_for_placement(46.0), "east");
    }

    #[test]
    fn return_ends_the_chunk() {
        // Lua 5.2: `return` must be a block's last statement, and it returns
        // from the whole chunk, so the outer `print(2)` never runs.
        assert_eq!(out("print(1) return"), ["1"]);
        assert!(out("do return end print(2)").is_empty());
    }
}
