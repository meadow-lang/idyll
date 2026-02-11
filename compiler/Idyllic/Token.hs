module Idyllic.Token (LToken, Token (..)) where

import Data.Text (Text)
import Idyllic.Utils

type LToken = Located Token

data Token
  = TokenError
  | TokenEOF
  | TokenTab
  | TokenNewline
  | TokenWhitespace
  | TokenIndent
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
  | TokenAt
  deriving (Show, Eq, Ord)
