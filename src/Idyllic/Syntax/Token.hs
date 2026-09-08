module Idyllic.Syntax.Token (LToken, Token (..), showToken) where

import Data.Text (Text)
import Data.Text qualified as T
import Idyllic.Utils

type LToken = Located Token

data Token
  = TokenError
  | TokenNewline
  | TokenUppercaseIdent Text
  | TokenLowercaseIdent Text
  | TokenOpIdent Text
  | TokenConOpIdent Text
  | TokenInt Integer
  | TokenString Text
  | TokenChar Char
  | TokenLParen
  | TokenRParen
  | TokenLBrace
  | TokenRBrace
  | TokenLBracket
  | TokenRBracket
  | TokenVLBrace
  | TokenVRBrace
  | TokenVSemi
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
  | TokenRFatArrow
  | TokenBar
  | TokenUnderscore
  | TokenAt
  | TokenMod
  | TokenUse
  | TokenData
  | TokenType
  | TokenLet
  | TokenIn
  | TokenWhere
  | TokenDo
  | TokenCase
  | TokenOf
  | TokenIf
  | TokenThen
  | TokenElse
  deriving (Show, Eq, Ord)

-- | A short, human-facing rendering of a token, used in parser error
--   messages (via the 'Text.Megaparsec.VisualStream' instance for the token
--   stream).
showToken :: Token -> String
showToken t = case t of
  TokenError -> "<lex-error>"
  TokenNewline -> "<newline>"
  TokenUppercaseIdent s -> T.unpack s
  TokenLowercaseIdent s -> T.unpack s
  TokenOpIdent s -> T.unpack s
  TokenConOpIdent s -> T.unpack s
  TokenInt n -> show n
  TokenString s -> show (T.unpack s)
  TokenChar c -> show c
  TokenLParen -> "("
  TokenRParen -> ")"
  TokenLBrace -> "{"
  TokenRBrace -> "}"
  TokenLBracket -> "["
  TokenRBracket -> "]"
  TokenVLBrace -> "{ (virtual)"
  TokenVRBrace -> "} (virtual)"
  TokenVSemi -> "; (virtual)"
  TokenBang -> "!"
  TokenHash -> "#"
  TokenBackSlash -> "\\"
  TokenColon -> ":"
  TokenSemi -> ";"
  TokenComma -> ","
  TokenPeriod -> "."
  TokenEq -> "="
  TokenLArrow -> "<-"
  TokenRArrow -> "->"
  TokenRFatArrow -> "=>"
  TokenBar -> "|"
  TokenUnderscore -> "_"
  TokenAt -> "@"
  TokenMod -> "mod"
  TokenUse -> "use"
  TokenData -> "data"
  TokenType -> "Type"
  TokenLet -> "let"
  TokenIn -> "in"
  TokenWhere -> "where"
  TokenDo -> "do"
  TokenCase -> "case"
  TokenOf -> "of"
  TokenIf -> "if"
  TokenThen -> "then"
  TokenElse -> "else"

-- instance Pretty Token where
--   pretty = \case
--     TokenError -> "ERROR"
--     TokenNewline -> "NEWLINE"
--     TokenUppercaseIdent t -> "UppercaseIdent(" <> pretty t <> ")"
--     TokenLowercaseIdent t -> "LowercaseIdent(" <> pretty t <> ")"
--     TokenOpIdent t -> "OpIdent(" <> pretty t <> ")"
--     TokenUse -> "Use"
--     TokenConOpIdent t -> "ConOpIdent(" <> pretty t <> ")"
--     TokenInt i -> "Int(" <> pretty i <> ")"
--     TokenString s -> "String(" <> pretty s <> ")"
--     TokenChar c -> "Char(" <> pretty c <> ")"
--     TokenLParen -> "LParen"
--     TokenRParen -> "RParen"
--     TokenLBrace -> "LBrace"
--     TokenRBrace -> "RBrace"
--     TokenLBracket -> "LBracket"
--     TokenRBracket -> "RBracket"
--     TokenVLBrace -> "VLBrace"
--     TokenVRBrace -> "VRBrace"
--     TokenVSemi -> "VSemi"
--     TokenBang -> "Bang"
--     TokenHash -> "Hash"
--     TokenBackSlash -> "BackSlash"
--     TokenColon -> "Colon"
--     TokenSemi -> "Semi"
--     TokenComma -> "Comma"
--     TokenPeriod -> "Period"
--     TokenEq -> "Eq"
--     TokenLArrow -> "LArrow"
--     TokenRArrow -> "RArrow"
--     TokenRFatArrow -> "RFatArrow"
--     TokenBar -> "Bar"
--     TokenUnderscore -> "Underscore"
--     TokenAt -> "At"
--     TokenMod -> "Mod"
--     TokenData -> "Data"
--     TokenType -> "Type"
--     TokenLet -> "Let"
--     TokenIn -> "In"
--     TokenWhere -> "Where"
--     TokenDo -> "Do"
--     TokenCase -> "Case"
--     TokenOf -> "Of"
--     TokenIf -> "If"
--     TokenThen -> "Then"
--     TokenElse -> "Else"