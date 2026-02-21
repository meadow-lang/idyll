module Idyllic.Layout where

import Idyllic.Token
import Idyllic.Utils

data Marked
  = Token LToken
  | OpenBlock Int Herald
  | Indent Int
  deriving (Show, Eq, Ord)

data Herald = IsHerald Token | NotHerald
  deriving (Show, Eq, Ord)

mark :: LineIndex -> [LToken] -> [Marked]
mark li = markIndents . markBlocks
  where
    markIndents :: [Marked] -> [Marked]
    markIndents (m@(OpenBlock _ _) : ms) = m : markIndents ms
    markIndents [Token n] | locItem n == TokenNewline = []
    markIndents (Token n : m@(Token t) : ms)
      | locItem n == TokenNewline =
          let (_, col) = offsetToLineCol li (spanStart $ locSpan t)
           in Indent col : m : markIndents ms
    markIndents (m : ms) = m : markIndents ms
    markIndents [] = []

    markBlocks :: [LToken] -> [Marked]
    markBlocks [t@(layoutHerald -> h@(IsHerald _))] = [Token t, OpenBlock 0 h]
    markBlocks (t@(layoutHerald -> h@(IsHerald _)) : n : ts) = case locItem n of
      TokenNewline -> case ts of
        [] -> [Token t, OpenBlock 0 h]
        (t' : ts') ->
          let (_, col) = offsetToLineCol li (spanStart $ locSpan t')
           in Token t : OpenBlock col h : Token t' : markBlocks ts'
      _ ->
        let (_, col) = offsetToLineCol li (spanStart $ locSpan n)
         in Token t : OpenBlock col h : Token n : markBlocks ts
    markBlocks (t : ts) = Token t : markBlocks ts
    markBlocks [] = []

    layoutHerald :: LToken -> Herald
    layoutHerald = go . locItem
      where
        go TokenLet = IsHerald TokenLet
        go TokenDo = IsHerald TokenDo
        go _ = NotHerald

layout :: LineIndex -> [LToken] -> [LToken]
layout li ts' = Located TokenVLBrace (Span 0 0) : go (mark li ts') [(1, NotHerald)]
  where
    go :: [Marked] -> [(Int, Herald)] -> [LToken]
    go (i@(Indent n) : ts) ((m, h) : ms)
      | m == n = Located TokenVSemi (Span m 1) : go ts ((m, h) : ms)
      | n < m = Located TokenVRBrace (Span m 2) : go (i : ts) ms
    go (Indent _ : ts) ms = go ts ms
    go (OpenBlock n h : ts) ((m, h') : ms)
      | n > m = Located TokenVLBrace (Span 0 3) : go ts ((n, h) : (m, h') : ms)
      | otherwise = error "nested context must be further indented than enclosing"
    go (OpenBlock n h : ts) []
      | n > 0 = Located TokenVLBrace (Span 0 4) : go ts [(n, h)]
    go (OpenBlock n _ : ts) ms = Located TokenVLBrace (Span 0 5) : Located TokenVRBrace (Span 0 7) : go (Indent n : ts) ms
    go (Token t : ts) ((0, NotHerald) : ms) | locItem t == TokenRBrace = t : go ts ms
    go (Token t : _) _ | locItem t == TokenRBrace = error "unexpected closing brace"
    go (Token t : ts) ms | locItem t == TokenLBrace = t : go ts ((0, NotHerald) : ms)
    go (Token t : ts) ((m, IsHerald kw) : ms) | m /= 0 && closesBlock (locItem t) kw = Located TokenVRBrace (Span m 7) : t : go ts ms
      where
        closesBlock TokenIn TokenLet = True
        closesBlock TokenWhere TokenDo = True
        closesBlock _ _ = False
    go (Token t : ts) ms = t : go ts ms
    go [] [] = []
    go [] ((m, _) : ms)
      | m /= 0 = Located TokenVRBrace (Span 0 8) : go [] ms
      | otherwise = error "not in layout context at end of input"
