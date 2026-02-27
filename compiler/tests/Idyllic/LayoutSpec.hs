{-# LANGUAGE TemplateHaskellQuotes #-}

module Idyllic.LayoutSpec where

import Data.Text (Text, unpack)
import Data.Text.IO qualified as T
import Data.Text.Lazy (toStrict)
import Idyllic.Layout (mark)
import Idyllic.Lexer (tokenize)
import Idyllic.Token (LToken)
import Idyllic.Utils (LineIndex, buildLineIndex)
import Test.Hspec (Spec, describe, it)
import Test.Hspec.Golden
import Text.Pretty.Simple (pShow)

myGoldenTest :: String -> Text -> Golden Text
myGoldenTest name actualOutput =
  Golden
    { output = actualOutput,
      encodePretty = prettyText,
      writeToFile = T.writeFile,
      readFromFile = T.readFile,
      goldenFile = ".golden/" <> name,
      actualFile = Just (".golden/" <> name <> "-actual"),
      failFirstTime = False
    }

prettyText :: Text -> String
prettyText = unpack . toStrict . pShow

spec :: Spec
spec = do
  testMark

layoutTestSetup :: Text -> ([LToken], LineIndex)
layoutTestSetup src = case tokenize src of
  Left es -> error $ "Tokenization failed: " ++ show es
  Right ts -> (ts, buildLineIndex src)

testMark :: Spec
testMark =
  describe "mark" $
    it "marks indents and opened blocks" $
      let (ts, li) = layoutTestSetup "some source code"
          out = toStrict . pShow $ mark li ts
       in myGoldenTest (show 'mark) out