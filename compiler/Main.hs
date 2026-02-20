module Main (main) where

import Data.Text
import Idyllic.Effect
import Idyllic.Pipeline (runPipelineIO)

src :: Text
src = "s f g x = f x (g x);\nk x y = x;\ni x = x;"

main :: IO ()
main = runPipelineIO False InputModeInteractive src
