module Main (main) where

import Data.Text
import Idyllic.Lexer
import Idyllic.Parser
import Text.Megaparsec
import Text.Pretty.Simple

src :: Text
src = "id x = x\nconst x y = x"

main :: IO ()
main = do
  case tokenize src of
    Left es -> print $ errorBundlePretty es
    Right ts -> case parseInteractive ts of
      Left es -> pPrint es
      Right ast -> pPrint ast
