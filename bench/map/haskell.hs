{-# LANGUAGE BangPatterns #-}
data List = Nil | Cons !Int List

build :: Int -> List -> List
build 0 acc = acc
build n acc = build (n - 1) (Cons n acc)

mapRev :: (Int -> Int) -> List -> List -> List
mapRev _ Nil acc = acc
mapRev f (Cons y ys) acc = mapRev f ys (Cons (f y) acc)
{-# NOINLINE mapRev #-}

sumL :: List -> Int -> Int
sumL Nil !acc = acc
sumL (Cons y ys) !acc = sumL ys (acc + y)

rounds :: Int -> Int -> Int
rounds 0 !total = total
rounds r !total = rounds (r - 1) (total + sumL (mapRev (\x -> x + r) (build 1000000 Nil) Nil) 0)

main :: IO ()
main = print (rounds 20 0)
