module Idyllic.Token (LToken, Token (..)) where

import Data.Text (Text)
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
  deriving (Show, Eq, Ord)
