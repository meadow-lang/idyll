{-# LANGUAGE TemplateHaskellQuotes #-}

module Idyllic.LayoutSpec where

import Data.Text (Text)
import Idyllic.Syntax.Layout (layout, mark)
import Idyllic.Syntax.Lexer (tokenize)
import Idyllic.Syntax.Token (LToken)
import Idyllic.Utils (LineIndex, buildLineIndex, prettyShowNoColor)
import Test.Hspec (Spec, after, describe, it)
import Test.Hspec.Golden

spec :: Spec
spec = do
  markSpec
  layoutSpec

markSpec :: Spec
markSpec =
  describe "mark" $ do
    mapM_ (uncurry goldenMark) inputs

layoutSpec :: Spec
layoutSpec =
  describe "layout" $ do
    mapM_ (uncurry goldenLayout) inputs

goldenMark :: String -> Text -> Spec
goldenMark name src =
  it ("correctly marks: " ++ name) $
    let (ts, _diags) = tokenize src
        li = buildLineIndex src
        out = prettyShowNoColor $ mark li ts
        fileName = show 'mark ++ "_" ++ name
     in defaultGolden fileName out

goldenLayout :: String -> Text -> Spec
goldenLayout name src =
  it ("correctly lays out: " ++ name) $
    let (ts, _diags) = tokenize src
        li = buildLineIndex src
        out = prettyShowNoColor $ fst $ layout li ts
        fileName = show 'layout ++ "_" ++ name
     in defaultGolden fileName out

inputs :: [(String, Text)]
inputs =
  [ ("single_line_let", "let x = 1 in x"),
    ( "multi_line_let",
      """
      let x = 1
          y = 2
       in y
      """
    ),
    ( "multi_line_let_nested",
      """
      let x = 1
          y = let j = 1
                  k = 2
               in j
          z = 3
       in add x y z
      """
    ),
    ( "multi_line_let_with_nested_single_line",
      """
      let x = 1
          y = let j = 1 in j
          z = 3 
       in add x y z
      """
    ),
    ( "multi_line_let_in_inline",
      """
      let x = 1
          y = 2 in y
      """
    ),
    ( "case_expression",
      """
      case xs of
        x -> 1
        y -> 2
      """
    ),
    ( "where_clause",
      """
      f x = x + y
        where
          y = 1
      """
    ),
    ( "nested_case_expression",
      """
      case x of
        1 -> 1
        y -> case y of
               2 -> 2
               z -> 3
      """
    ),
    ( "do_let",
      """
      do
        let x = 1
            y = 2
        pure y 
      """
    ),
    ( "case_multi_line_let",
      """
      case x of
        1 -> 1
        y -> let z = 2 
                 g = 3
              in z
      """
    ),
    ( "let_case_let",
      """
      let x = case y of
            1 -> 1
            z -> let g = 2 
                     b = 3
                  in g
            f -> 4
        in x      """
    ),
    ( "do_let_in",
      """
      do
        let x = 1 in pure x
        5
      """
    ),
    ( "let_do_let_in",
      """
      let x = do
            let y = True
            y
       in x
      """
    ),
    ( "let_do_let_with_do_call",
      """
      foo = do
        let s = f ", " $ map show $ do
             x <- [1,2,3,4]
             return (x, y)
        putStrLn s
        return ()
      """
    )
  ]
