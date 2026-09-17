-- | Checks the Unicode port against output recorded from the Rust crates
--   (see gen/regen.sh): per-character widths, string widths and grapheme
--   boundaries for ~15k strings, including every 3-character combination of
--   an alphabet chosen to exercise the tricky rules.
module System.Console.Rustyline.UnicodeSpec (spec) where

import Control.Monad (forM_, unless)
import Data.Char (chr)
import Data.Text qualified as T
import Data.Text.IO qualified as TIO
import Numeric (readHex)
import System.Console.Rustyline.Internal.Unicode.Grapheme
import System.Console.Rustyline.Internal.Unicode.Width
import Test.Hspec

data Sample = Sample
  { sText :: T.Text,
    sWidth :: Int,
    sCharWidths :: [Maybe Int],
    sStarts :: [Int]
  }

parseSample :: T.Text -> Sample
parseSample line = case T.splitOn "|" line of
  [cps, w, cws, grs] ->
    Sample
      { sText = T.pack (map (chr . hex) (fields cps)),
        sWidth = read (T.unpack w),
        sCharWidths = map cw (fields cws),
        sStarts = map (read . T.unpack) (fields grs)
      }
  _ -> error ("bad oracle line: " <> T.unpack line)
  where
    fields t = filter (not . T.null) (T.splitOn "," t)
    hex t = case readHex (T.unpack t) of
      [(n, "")] -> n
      _ -> error "bad hex"
    cw "N" = Nothing
    cw t = Just (read (T.unpack t))

spec :: Spec
spec = describe "Unicode tables" $ do
  samples <- runIO (map parseSample . T.lines <$> TIO.readFile "test/data/unicode-oracle.txt")
  it "loads the oracle" $ length samples `shouldSatisfy` (> 15000)
  it "matches unicode-width char widths" $
    forM_ samples $ \s ->
      map charWidth (T.unpack (sText s)) `shouldBe` sCharWidths s
  it "matches unicode-width string widths" $
    forM_ samples $ \s ->
      unless (strWidth (sText s) == sWidth s) $
        expectationFailure (show (sText s) <> ": expected width " <> show (sWidth s) <> ", got " <> show (strWidth (sText s)))
  it "matches unicode-segmentation grapheme boundaries" $
    forM_ samples $ \s ->
      unless (map fst (graphemeIndices (sText s)) == sStarts s) $
        expectationFailure (show (sText s) <> ": expected " <> show (sStarts s) <> ", got " <> show (map fst (graphemeIndices (sText s))))
