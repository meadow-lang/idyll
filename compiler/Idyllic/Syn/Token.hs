module Idyllic.Syn.Token
  ( Token (..),
    TokenKind (..),
    isTrivia,
    isSpace,
    isKeyword,
    isLayoutKeyword,
  )
where

import Data.ByteString (ByteString)
import Idyllic.Utils.Span (Span, slice)

data Token = Token {tokenKind :: TokenKind, tokenSpan :: !Span}
  deriving (Show, Eq, Ord)

data TokenKind
  = TokenKindError
  | TokenKindTab
  | TokenKindNewline
  | TokenKindWhitespace
  | TokenKindUppercaseIdent
  | TokenKindLowercaseIdent
  | TokenKindOpIdent
  | TokenKindConOpIdent
  | TokenKindInt
  | TokenKindString
  | TokenKindChar
  | TokenKindLParen
  | TokenKindRParen
  | TokenKindLBrace
  | TokenKindRBrace
  | TokenKindLBracket
  | TokenKindRBracket
  | TokenKindBang
  | TokenKindHash
  | TokenKindBackSlash
  | TokenKindColon
  | TokenKindSemi
  | TokenKindComma
  | TokenKindPeriod
  | TokenKindEq
  | TokenKindLArrow
  | TokenKindRArrow
  | TokenKindLFatArrow
  | TokenKindBar
  | TokenKindUnderscore
  deriving (Show, Eq, Ord)

isTrivia :: Token -> Bool
isTrivia = go . tokenKind
  where
    go TokenKindWhitespace = True
    go TokenKindNewline = True
    go TokenKindTab = True
    go _ = False

isSpace :: Token -> Bool
isSpace = go . tokenKind
  where
    go TokenKindWhitespace = True
    go _ = False

tokenText :: Token -> ByteString -> ByteString
tokenText t = slice $ tokenSpan t

isKeyword :: Token -> ByteString -> Bool
isKeyword t src = case tokenKind t of
  TokenKindLowercaseIdent -> go $ tokenText t src
  _ -> False
  where
    go "use" = True
    go "let" = True
    go "in" = True
    go "where" = True
    go "if" = True
    go "then" = True
    go "else" = True
    go "match" = True
    go "with" = True
    go "record" = True
    go "data" = True
    go "type" = True
    go _ = False

isLayoutKeyword :: Token -> ByteString -> Bool
isLayoutKeyword t src = case tokenKind t of
  TokenKindLowercaseIdent -> go $ tokenText t src
  _ -> False
  where
    go "let" = True
    go "where" = True
    go "with" = True
    go _ = False
