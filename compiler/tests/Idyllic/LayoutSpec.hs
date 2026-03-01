{-# LANGUAGE TemplateHaskellQuotes #-}

module Idyllic.LayoutSpec where

import Data.Text (Text)
import Idyllic.Layout (mark)
import Idyllic.Lexer (tokenize)
import Idyllic.Token (LToken)
import Idyllic.Utils (LineIndex, buildLineIndex, prettyShowNoColor)
import Test.Hspec (Spec, describe, it)
import Test.Hspec.Golden

spec :: Spec
spec = do
  markSpec

markSpec :: Spec
markSpec =
  describe "mark" $ do
    goldenMark "single_line_let" "let x = 1 in x"
    goldenMark
      "multi_line_let"
      """
      let x = 1
          y = 2
       in y
      """
    goldenMark
      "multi_line_let_nested"
      """
      let x = 1
          y = let j = 1
                  k = 2
               in j
          z = 3
       in add x y z
      """
    goldenMark
      "multi_line_let_with_nested_single_line"
      """
      let x = 1
          y = let j = 1 in j
          z = 3 
       in add x y z
      """
    goldenMark
      "multi_line_let_in_inline"
      """
      let x = 1
          y = 2 in y
      """
    goldenMark
      "case_expression"
      """
      case xs of
        x -> 1
        y -> 2
      """
    goldenMark
      "where_clause"
      """
      f x = x + y
        where
          y = 1
      """
    goldenMark
      "nested_case_expression"
      """
      case x of
        1 -> 1
        y -> case y of
               2 -> 2
               z -> 3
      """
    goldenMark
      "do_let"
      """
      do
        let x = 1
            y = 2
        pure y 
      """

goldenMark :: String -> Text -> Spec
goldenMark name src =
  it ("correctly marks: " ++ name) $
    let (ts, li) = case tokenize src of
          Left es -> error $ "Tokenization failed: " ++ show es
          Right ts' -> (ts', buildLineIndex src)
        out = prettyShowNoColor $ mark li ts
        fileName = show 'mark ++ "_" ++ name
     in defaultGolden fileName out