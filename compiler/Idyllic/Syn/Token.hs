module Idyllic.Syn.Token
  ( LToken,
    Token (..),
    tokenText,
    -- trivia,
    -- kw,
    -- punct,
    -- isTrivia,
    -- isSpace,
    -- isKeyword,
    -- isLayoutKeyword,
  )
where

import Data.ByteString (ByteString)
import Idyllic.Utils.Located
import Idyllic.Utils.Span (slice)

type LToken = Located Token

data Token
  = TokenError
  | TokenEOF
  | TokenTab
  | TokenNewline
  | TokenWhitespace
  | TokenUppercaseIdent
  | TokenLowercaseIdent
  | TokenOpIdent
  | TokenConOpIdent
  | TokenInt
  | TokenString
  | TokenChar
  | TokenLParen
  | TokenRParen
  | TokenLBrace
  | TokenRBrace
  | TokenLBracket
  | TokenRBracket
  | TokenBang
  | TokenHash
  | TokenBackSlash
  | TokenColon
  | TokenSemi
  | TokenComma
  | TokenPeriod
  | TokenEq
  | TokenLArrow
  | TokenRArrow
  | TokenLFatArrow
  | TokenBar
  | TokenUnderscore
  deriving (Show, Eq, Ord)

tokenText :: LToken -> ByteString -> ByteString
tokenText t = slice $ locSpan t

-- trivia :: Text -> Token
-- trivia "\t" = TokenTab
-- trivia "\n" = TokenNewline
-- trivia "<WS>" = TokenWhitespace
-- trivia "<EOF>" = TokenEOF
-- trivia _ = TokenError

-- kw :: Text -> Token
-- kw "use" = TokenLowercaseIdent "use"
-- kw "let" = TokenLowercaseIdent "let"
-- kw "in" = TokenLowercaseIdent "in"
-- kw "where" = TokenLowercaseIdent "where"
-- kw "if" = TokenLowercaseIdent "if"
-- kw "then" = TokenLowercaseIdent "then"
-- kw "else" = TokenLowercaseIdent "else"
-- kw "case" = TokenLowercaseIdent "case"
-- kw "of" = TokenLowercaseIdent "of"
-- kw "record" = TokenLowercaseIdent "record"
-- kw "data" = TokenLowercaseIdent "data"
-- kw "type" = TokenLowercaseIdent "type"
-- kw _ = TokenError

-- punct :: Text -> Token
-- punct "(" = TokenLParen
-- punct ")" = TokenRParen
-- punct "{" = TokenLBrace
-- punct "}" = TokenRBrace
-- punct "[" = TokenLBracket
-- punct "]" = TokenRBracket
-- punct "!" = TokenBang
-- punct "#" = TokenHash
-- punct "\\" = TokenBackSlash
-- punct ":" = TokenColon
-- punct ";" = TokenSemi
-- punct "," = TokenComma
-- punct "." = TokenPeriod
-- punct "=" = TokenEq
-- punct "<-" = TokenLArrow
-- punct "->" = TokenRArrow
-- punct "=>" = TokenLFatArrow
-- punct "|" = TokenBar
-- punct "_" = TokenUnderscore
-- punct _ = TokenError

-- isTrivia :: Token -> Bool
-- isTrivia = go . Token
--   where
--     go TokenWhitespace = True
--     go TokenNewline = True
--     go TokenTab = True
--     go _ = False

-- isSpace :: Token -> Bool
-- isSpace = go . Token
--   where
--     go TokenWhitespace = True
--     go _ = False

-- isKeyword :: Token -> ByteString -> Bool
-- isKeyword t src = case Token t of
--   TokenLowercaseIdent -> go $ tokenText t src
--   _ -> False
--   where
--     go "use" = True
--     go "let" = True
--     go "in" = True
--     go "where" = True
--     go "if" = True
--     go "then" = True
--     go "else" = True
--     go "case" = True
--     go "of" = True
--     go "record" = True
--     go "data" = True
--     go "type" = True
--     go _ = False

-- isLayoutKeyword :: Token -> ByteString -> Bool
-- isLayoutKeyword t src = case Token t of
--   TokenLowercaseIdent -> go $ tokenText t src
--   _ -> False
--   where
--     go "let" = True
--     go "where" = True
--     go "of" = True
--     go "do" = True
--     go _ = False
