module Idyllic.Token
  ( LToken,
    Token (..),
    tokenText,
  )
where

import Data.ByteString (ByteString)
import Idyllic.Utils

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
  | TokenAt
  deriving (-- | TokenWhere
            -- | TokenLet
            -- | TokenIn
            -- | TokenIf
            -- | TokenThen
            -- | TokenElse
            -- | TokenCase
            -- | TokenOf
            -- | TokenData
            -- | TokenRecord
            -- | TokenClass
            -- | TokenInstance
            -- | TokenMod
            -- | TokenUse
            -- | TokenAs
            -- | TokenEnd
            Show, Eq, Ord)

tokenText :: LToken -> ByteString -> ByteString
tokenText t = slice $ locSpan t
