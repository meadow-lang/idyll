{-# LANGUAGE BangPatterns #-}
-- a strict list of our own, so that GHC cannot fuse the list away
data List = Nil | Cons !Int List

build :: Int -> List -> List
build 0 acc = acc
build n acc = build (n - 1) (Cons n acc)

sumL :: List -> Int -> Int
sumL Nil !acc = acc
sumL (Cons y ys) !acc = sumL ys (acc + y)

rounds :: Int -> Int -> Int
rounds 0 !total = total
rounds r !total = rounds (r - 1) (total + sumL (build (1000000 + r) Nil) 0)

main :: IO ()
main = print (rounds 20 0)
