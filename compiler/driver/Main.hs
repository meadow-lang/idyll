module Main (main) where

import Idyllic.Effect
import Idyllic.Pipeline (runPipelineIO)

main :: IO ()
main = runPipelineIO False InputModeInteractive "s f g x = f x (g x)"
