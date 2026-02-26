module Main (main) where

import Data.Text
import Idyllic.Effect
import Idyllic.Pipeline (runPipelineIO)

src :: Text
-- src = "s f g x = f x (g x)\nk x y = x\ni x = x"

-- src = "let x = 1 in x"
src = "do\n  let x = 1\n      y = 2\n  pure y"
-- src =
--   """
--   do
--     let x = 1
--         y = 2
--     pure y 
--   """

-- src =
--   """
--   let x = 1
--       y = let j = 1
--               k = 2
--            in j
--       z = 3
--    in add x y z
--   """

-- a = let x = 1
--         y = let j = 1
--                 k = 2
--              in j
--         z = 3
--    in add x y z

-- """
-- let x = 1
--     y = 2 in y
-- """

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
--     h -> 5
--   """

main :: IO ()
main = runPipelineIO False InputModeInteractive src

-- xs = [1, 2, 3]

-- p = case xs of
--   [] -> 1
--   (y : ys) -> case y of
--     2 -> 3
--     f -> z
--       where
--         z = 1
--     g -> 3
--   h -> 5

-- p' = case xs of {
--   [] -> 1;
--   (y:ys) -> case y of {
--     2 -> 3;
--     f -> z where {z = 1};
--     g -> 4
--   };
--   h -> 5
-- }

-- m = do
--   case True of
--     True -> pure x
--       where x = 1
--   putStrLn "I am still in the do block!"

-- m :: IO ()
-- m = do
--   let x = 1
--   let y = z
--     where z = 2
--   pure ()