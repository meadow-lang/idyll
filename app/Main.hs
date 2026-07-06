module Main (main) where

import Idyllic.Effect
import Idyllic.Pipeline (runPipelineIO)

main :: IO ()
main = runPipelineIO False InputModeInteractive "let s f g x = f x (g x) in s f g 5"
