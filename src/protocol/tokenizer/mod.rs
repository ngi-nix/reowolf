enum Token {
    
}

enum TokenRangeType {
    Pragma,
    Import,
    Defintion,
}

struct TokenRange {
    range_type: TokenRangeType,
    start: usize,
    end: usize,
}

struct TokenBuffer {
    tokens: Vec<Token>,
    ranges: Vec<TokenRange>,
}

