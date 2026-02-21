module Idyllic.Layout where

import Data.Text (Text)
import Idyllic.Token
import Idyllic.Utils

data Delim
  = Parens
  | Braces
  | Brackets
  | Layout Herald
  deriving (Show, Eq, Ord)

data Herald = Let | Where | Do | Of deriving (Show, Eq, Ord)

toHerald :: LToken -> Herald
toHerald t = case locItem t of
  TokenLowercaseIdent "let" -> Let
  TokenLowercaseIdent "where" -> Where
  TokenLowercaseIdent "do" -> Do
  TokenLowercaseIdent "of" -> Of
  _ -> error "Not a herald"

data Marked
  = Token LToken
  | OpenBlock Int Herald
  | Indent Int
  deriving (Show, Eq, Ord)

isLayoutHerald :: LToken -> Bool
isLayoutHerald = go . locItem
  where
    go (TokenLowercaseIdent n) = n `elem` ["let", "where", "do", "of"]
    go _ = False

mark :: LineIndex -> [LToken] -> [Marked]
mark li = markIndents li . markBlocks li

markIndents :: LineIndex -> [Marked] -> [Marked]
markIndents li (m@(OpenBlock _ _) : ms) = m : markIndents li ms
markIndents _ [Token n] | locItem n == TokenNewline = []
markIndents li (Token n : m@(Token t) : ms)
  | locItem n == TokenNewline =
      let (_, col) = offsetToLineCol li (spanStart $ locSpan t)
       in Indent col : m : markIndents li ms
markIndents li (m : ms) = m : markIndents li ms
markIndents _ [] = []

markBlocks :: LineIndex -> [LToken] -> [Marked]
markBlocks _ [t] | isLayoutHerald t = [Token t, OpenBlock 0 (toHerald t)]
markBlocks li (t : n : ts)
  | isLayoutHerald t = case locItem n of
      TokenNewline -> case ts of
        [] -> [Token t, OpenBlock 0 (toHerald t)]
        (t' : ts') ->
          let (_, col) = offsetToLineCol li (spanStart $ locSpan t')
           in Token t : OpenBlock col (toHerald t) : Token t' : markBlocks li ts'
      _ ->
        let (_, col) = offsetToLineCol li (spanStart $ locSpan n)
         in Token t : OpenBlock col (toHerald t) : Token n : markBlocks li ts
markBlocks li (t : ts) = Token t : markBlocks li ts
markBlocks _ [] = []

layDKillaz :: [(Herald, Text)]
layDKillaz = [(Do, "where"), (Let, "in")]

layout :: LineIndex -> [LToken] -> [LToken]
layout li ts' = Located TokenVLBrace (Span 0 0) : go (mark li ts') [(1, Just Where)]
  where
    go :: [Marked] -> [(Int, Maybe Herald)] -> [LToken]
    go (i@(Indent n) : ts) ((m, h) : ms)
      | m == n = Located TokenVSemi (Span m 1) : go ts ((m, h) : ms)
      | n < m = Located TokenVRBrace (Span m 2) : go (i : ts) ms
    go (Indent _ : ts) ms = go ts ms
    go (OpenBlock n h : ts) ((m, h') : ms)
      | n > m = Located TokenVLBrace (Span 0 3) : go ts ((n, Just h) : (m, h') : ms)
      | otherwise = error "nested context must be further indented than enclosing"
    go (OpenBlock n h : ts) []
      | n > 0 = Located TokenVLBrace (Span 0 4) : go ts [(n, Just h)]
    -- \| otherwise = error "nested context must be further indented than enclosing"
    go (OpenBlock n _ : ts) ms = Located TokenVLBrace (Span 0 5) : Located TokenVRBrace (Span 0 7) : go (Indent n : ts) ms
    go (Token t : ts) ((0, Nothing) : ms) | locItem t == TokenRBrace = t : go ts ms
    go (Token t : _) _ | locItem t == TokenRBrace = error "unexpected closing brace"
    go (Token t : ts) ms | locItem t == TokenLBrace = t : go ts ((0, Nothing) : ms)
    go (Token t : ts) ((m, Just h) : ms) | m /= 0 && closesBlock t h = Located TokenVRBrace (Span m 7) : t : go ts ms
      where
        closesBlock t' hd = closesHerald (locItem t')
          where
            closesHerald (TokenLowercaseIdent name) =
              case lookup hd layDKillaz of
                Nothing -> False
                Just closer -> name == closer
            closesHerald _ = False
    go (Token t : ts) ms = t : go ts ms
    go [] [] = []
    go [] ((m, _) : ms)
      | m /= 0 = Located TokenVRBrace (Span 0 8) : go [] ms
      | otherwise = error "not in layout context at end of input"
