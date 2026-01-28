module Idyllic.Syn.Layout (layout) where

import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import qualified Data.ByteString.Char8 as BSC
import Idyllic.Syn.Token
import Idyllic.Utils.LineIndex (LineIndex, offsetToLineCol)
import Idyllic.Utils.Located
import Idyllic.Utils.Span (Span (..))

layout :: LineIndex -> ByteString -> [Token] -> [Token]
layout lineIndex src = undefined
  where

-- go :: [Int] -> [Token] -> [Token]
-- go stack@(m : ms) (tNL : ts)
--   | tokenKind tNL == TokenKindNewline =
--       let col = getColumn lineIndex tNL
--        in case compare col m of
--             EQ -> Located TokenSemi (Span 0 0) : go stack ts
--             GT -> go ms ts
--             LT -> Located TokenRBrace (Span 0 0) : go ms (Located TokenNewline (Span 0 0) : ts)
-- go (m : ms) (t : c : ts)
--   | isLayoutKeyword t src && tokenKind c == TokenKindColon =
--       case span isTrivia ts of
--         (_, h : rest) ->
--           let col = getColumn lineIndex h
--            in if col > m
--                 then Located TokenLBrace (Span 0 0) : go (col : m : ms) (h : rest)
--                 else error "Layout error: expected indented block after layout keyword"
--         _ -> error "Shouldn't be possible to have a layout keyword not followed by any tokens"
-- go [] (t : c : ts)
--   | isLayoutKeyword t src && tokenKind c == TokenKindColon =
--       case span isTrivia ts of
--         (_, h : rest) ->
--           let col = getColumn lineIndex h
--            in if col > 1
--                 then Located TokenLBrace (Span 0 0) : go [col] (Located TokenLBrace (Span 0 0) : h : rest)
--                 else error "Layout error: expected indented block after layout keyword"
--         _ -> error "Shouldn't be possible to have a layout keyword not followed by any tokens"
-- -- go ms (t : ts)
-- --   | tokenKind t == TokenKindLBrace =
-- go ms (t : ts) = t : go ms ts
-- go [] [] = []
-- go (m : ms) []
--   | m == 0 = error "Shouldn't be possible to be in a non-layout context at EOF"
--   | otherwise = Located TokenRBrace (Span 0 0) : go ms []

-- terminatesLayoutBlock :: Token -> Token -> Bool
-- terminatesLayoutBlock layoutTok tok = undefined

-- -- Layout keyword specific terminators
-- terminators :: [(Token, [Token])]
-- terminators =
--   [ (kw "let", [kw "in"]),
--     (kw "where", []),
--     (kw "do", []),
--     (kw "of", [kw "where"])
--   ]

-- -- Tokens terminate any layout block
-- commonTerminators :: [Token]
-- commonTerminators =
--   [ punct ";",
--     punct "}",
--     punct ")",
--     punct "]",
--     TokenEOF
--   ]

getColumn :: LineIndex -> LToken -> Int
getColumn li tok = snd $ offsetToLineCol li startOffset
  where
    startOffset = spanStart (locSpan tok)