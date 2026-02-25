module Idyllic.Layout where

import Idyllic.Token
import Idyllic.Utils

data LayoutError
  = NestedLayoutContextError Int Int Span
  | UnexpectedClosingBraceError Span
  | NotInLayoutContextAtEndOfInputError Span
  deriving (Show, Eq, Ord)

data Marked
  = Token LToken
  | OpenBlock Int (Maybe Token) Span
  | Indent Int Span
  deriving (Show, Eq, Ord)

mark :: LineIndex -> [LToken] -> [Marked]
mark li = markIndents . markBlocks
  where
    skipNewlines :: [LToken] -> [LToken]
    skipNewlines (Located TokenNewline _ : ts) = dropWhile isNewline ts
      where
        isNewline (Located TokenNewline _) = True
        isNewline _ = False
    skipNewlines ts = ts

    markIndents :: [Marked] -> [Marked]
    markIndents (m@(OpenBlock {}) : ms) = m : markIndents ms
    markIndents [Token n] | unLoc n == TokenNewline = []
    markIndents (Token n : m@(Token t) : ms)
      | unLoc n == TokenNewline =
          let (_, col) = offsetToLineCol li (spanStart $ locSpan t)
           in Indent col (locSpan n) : m : markIndents ms
    markIndents (m : ms) = m : markIndents ms
    markIndents [] = []

    markBlocks :: [LToken] -> [Marked]
    markBlocks [t@(layoutHerald -> h@(Just _))] = [Token t, OpenBlock 0 h (locSpan t)]
    markBlocks (t@(layoutHerald -> h@(Just _)) : n : ts) | unLoc n /= TokenLBrace = case unLoc n of
      TokenNewline -> case skipNewlines ts of
        [] -> [Token t, OpenBlock 0 h (locSpan t)]
        (t' : ts') ->
          let (_, col) = offsetToLineCol li (spanStart $ locSpan t')
           in Token t : OpenBlock col h (locSpan t) : Token t' : markBlocks ts'
      _ ->
        let (_, col) = offsetToLineCol li (spanStart $ locSpan n)
         in Token t : OpenBlock col h (locSpan t) : Token n : markBlocks ts
    markBlocks (t : ts) = Token t : markBlocks ts
    markBlocks [] = []

    layoutHerald :: LToken -> Maybe Token
    layoutHerald = go . unLoc
      where
        go TokenLet = Just TokenLet
        go TokenDo = Just TokenDo
        go _ = Nothing

layout :: LineIndex -> [LToken] -> ([LToken], [LayoutError])
layout li ts' =
  let (ts, es) = go (mark li ts') [(1, Nothing)]
   in (Located TokenVLBrace (Span 0 0) : ts, es)
  where
    go :: [Marked] -> [(Int, Maybe Token)] -> ([LToken], [LayoutError])
    go (i@(Indent n s) : ts) ((m, h) : ms)
      | m == n = let (ts'', es) = go ts ((m, h) : ms) in (Located TokenVSemi s : ts'', es)
      -- \| n < m = let (ts'', es) = go (i : ts) ms in (Located TokenVRBrace s : ts'', es)
      | n < m = let (ts'', es) = go (i : ts) ms in (Located TokenVRBrace (Span m 1) : ts'', es)
    go (Indent {} : ts) ms = go ts ms
    go (OpenBlock n h s : ts) ((m, h') : ms)
      | n > m = let (ts'', es) = go ts ((n, h) : (m, h') : ms) in (Located TokenVLBrace s : ts'', es)
      | otherwise = let (ts'', es) = go ts ms in (ts'', NestedLayoutContextError m n s : es)
    go (OpenBlock n h s : ts) []
      | n > 0 = let (ts'', es) = go ts [(n, h)] in (Located TokenVLBrace s : ts'', es)
    go (OpenBlock n _ s : ts) ms = let (ts'', es) = go (Indent n s : ts) ms in (Located TokenVLBrace s : Located TokenVRBrace s : ts'', es)
    go (Token t : ts) ((0, Nothing) : ms) | unLoc t == TokenRBrace = let (ts'', es) = go ts ms in (t : ts'', es)
    go (Token t : Token kw : ts) ((0, Just h) : ms) | unLoc t == TokenRBrace && closesBlock (unLoc kw) h = let (ts'', es) = go ts ms in (t : kw : ts'', es)
    go (Token t : ts) ms | unLoc t == TokenRBrace = let (ts'', es) = go ts ms in (ts'', UnexpectedClosingBraceError (locSpan t) : es)
    go (Token kw : Token lb : ts) ms
      | unLoc lb == TokenLBrace && isHerald (unLoc kw) =
          let (ts'', es) = go ts ((0, Just (unLoc kw)) : ms) in (kw : lb : ts'', es)
      where
        isHerald TokenLet = True
        isHerald TokenDo = True
        isHerald _ = False
    go (Token t : ts) ms | unLoc t == TokenLBrace = let (ts'', es) = go ts ((0, Nothing) : ms) in (t : ts'', es)
    go (Token t : ts) ((m, Just kw) : ms)
      | m /= 0 && closesBlock (unLoc t) kw =
          -- let (ts'', es) = go ts ms in (Located TokenVRBrace (locSpan t) : t : ts'', es)
          let (ts'', es) = go ts ms in (Located TokenVRBrace (Span m 2) : t : ts'', es)
    go (Token t : ts) ms = let (ts'', es) = go ts ms in (t : ts'', es)
    go [] [] = ([], [])
    go [] ((m, _) : ms)
      | m /= 0 = let (ts'', es) = go [] ms in (Located TokenVRBrace (lastSpan ts') : ts'', es)
      | otherwise = ([], [NotInLayoutContextAtEndOfInputError (lastSpan ts')])
      where
        lastSpan [] = Span 0 0
        lastSpan [t] = locSpan t
        lastSpan (_ : rst) = lastSpan rst

closesBlock :: Token -> Token -> Bool
closesBlock TokenIn TokenLet = True
closesBlock TokenWhere TokenDo = True
closesBlock _ _ = False

foo :: IO Int
foo = do
  let x = 1
      y = 2
  pure y