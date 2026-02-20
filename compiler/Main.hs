module Main (main) where

import Data.Text
import Idyllic.Effect
import Idyllic.Pipeline (runPipelineIO)

src :: Text
src = "s f g x = f x (g x)\nk x y = x\ni x = x"

-- src = "let x = 1 in x"

-- src =
--   """
--   let x = 1
--       y = let j = 1 in j
--       z = 3
--    in x + y + z
--   """

-- src =
--   """
--   case xs of
--     x -> 1
--     y -> 2
--   """

-- src =
--   """

--   where
--     x = 1
--     y = 2
--   """

-- src =
--   """
--   case x of
--     1 -> 1
--     y -> case y of
--       2 -> 3
--       f -> z
--         where z = 1
--       g -> 4
--   """

main :: IO ()
main = runPipelineIO False InputModeInteractive src

-- x = case x of
--   1 -> 1
--   y -> case y of
--     2 -> 3
--     f -> z
--       where
--         z = 1
--     g -> 4

-- x = case x of {
--   1 -> 1;
--   y -> case y of {
--     2 -> 3;
--     f -> z
--     } where {z = 1};
--     g -> 4
-- }
