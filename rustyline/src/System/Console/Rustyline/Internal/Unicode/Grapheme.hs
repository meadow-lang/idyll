-- | Extended grapheme cluster segmentation (UAX #29), ported from the
--   @unicode-segmentation@ 1.13.3 crate's @GraphemeCursor@.
--
--   rustyline only ever segments whole string slices, so this port keeps the
--   crate's boundary rules (including GB9c, GB11 and GB12/13, which look
--   backwards) but drops its chunked-input machinery. Offsets are UTF-8 byte
--   offsets, like the Rust API.
module System.Console.Rustyline.Internal.Unicode.Grapheme
  ( graphemes,
    graphemeIndices,
    graphemeCount,
  )
where

import Data.Char (ord)
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Internal.Text (charLenUtf8, lenB, sliceB)
import System.Console.Rustyline.Internal.Unicode.RangeTable
import System.Console.Rustyline.Internal.Unicode.Tables

-- Keep the constructor order in sync with GC_ORDER in gen/gen_tables.py.
data GraphemeCat
  = GCAny
  | GCCR
  | GCControl
  | GCExtend
  | GCExtendedPictographic
  | GCInCBConsonant
  | GCL
  | GCLF
  | GCLV
  | GCLVT
  | GCPrepend
  | GCRegionalIndicator
  | GCSpacingMark
  | GCT
  | GCV
  | GCZWJ
  deriving (Eq, Enum, Show)

graphemeCategory :: Char -> GraphemeCat
graphemeCategory c = toEnum (lookupRange graphemeCatTable 0 (ord c))

isIncbLinker :: Char -> Bool
isIncbLinker c = inRange incbLinkerTable (ord c)

isIncbExtend :: Char -> Bool
isIncbExtend c = inRange incbExtendTable (ord c)

data PairResult
  = NotBreak
  | Break
  | Extended
  | InCbConsonant
  | Regional
  | Emoji

checkPair :: GraphemeCat -> GraphemeCat -> PairResult
checkPair before after = case (before, after) of
  (GCCR, GCLF) -> NotBreak -- GB3
  (b, _) | b `elem` [GCControl, GCCR, GCLF] -> Break -- GB4
  (_, a) | a `elem` [GCControl, GCCR, GCLF] -> Break -- GB5
  (GCL, a) | a `elem` [GCL, GCV, GCLV, GCLVT] -> NotBreak -- GB6
  (b, a) | b `elem` [GCLV, GCV] && a `elem` [GCV, GCT] -> NotBreak -- GB7
  (b, GCT) | b `elem` [GCLVT, GCT] -> NotBreak -- GB8
  (_, a) | a `elem` [GCExtend, GCZWJ] -> NotBreak -- GB9
  (_, GCSpacingMark) -> Extended -- GB9a
  (GCPrepend, _) -> Extended -- GB9b
  (_, GCInCBConsonant) -> InCbConsonant -- GB9c
  (GCZWJ, GCExtendedPictographic) -> Emoji -- GB11
  (GCRegionalIndicator, GCRegionalIndicator) -> Regional -- GB12, GB13
  _ -> Break -- GB999

-- | Is there a boundary between the text before (given reversed, non-empty)
--   and the character after? Extended clusters, so GB9a/GB9b never break.
isBoundary :: [Char] -> Char -> Bool
isBoundary [] _ = True
isBoundary prev@(b : _) a = case checkPair (graphemeCategory b) (graphemeCategory a) of
  NotBreak -> False
  Break -> True
  Extended -> False
  InCbConsonant -> incbConsonant 0 prev
  Regional -> even (length (takeWhile ((== GCRegionalIndicator) . graphemeCategory) prev))
  Emoji -> emoji prev
  where
    -- GB9c: Consonant [Extend Linker]* Linker [Extend Linker]* × Consonant
    incbConsonant :: Int -> [Char] -> Bool
    incbConsonant _ [] = True
    incbConsonant linkers (ch : rest)
      | isIncbLinker ch = incbConsonant (linkers + 1) rest
      | isIncbExtend ch = incbConsonant linkers rest
      | otherwise = not (linkers > 0 && graphemeCategory ch == GCInCBConsonant)

    -- GB11: ExtPict Extend* ZWJ × ExtPict
    emoji (z : rest)
      | graphemeCategory z /= GCZWJ = True
      | otherwise = afterZwj rest
    emoji [] = True
    afterZwj (ch : rest) = case graphemeCategory ch of
      GCExtend -> afterZwj rest
      GCExtendedPictographic -> False
      _ -> True
    afterZwj [] = True

-- | Start offsets (UTF-8 bytes) of the grapheme clusters of the text.
graphemeStarts :: Text -> [Int]
graphemeStarts t = case T.unpack t of
  [] -> []
  (c0 : cs) -> 0 : go [c0] (charLenUtf8 c0) cs
  where
    go _ _ [] = []
    go prev off (c : rest)
      | isBoundary prev c = off : go (c : prev) (off + charLenUtf8 c) rest
      | otherwise = go (c : prev) (off + charLenUtf8 c) rest

-- | Clusters paired with their UTF-8 byte offsets
--   (@UnicodeSegmentation::grapheme_indices(s, true)@).
graphemeIndices :: Text -> [(Int, Text)]
graphemeIndices t = zipWith slice starts (drop 1 starts ++ [end])
  where
    starts = graphemeStarts t
    end = lenB t
    slice s e = (s, sliceB s e t)

-- | The extended grapheme clusters of the text.
graphemes :: Text -> [Text]
graphemes = map snd . graphemeIndices

graphemeCount :: Text -> Int
graphemeCount = length . graphemeStarts
