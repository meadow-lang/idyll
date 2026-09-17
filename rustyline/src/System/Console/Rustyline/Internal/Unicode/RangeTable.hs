-- | Sorted, non-overlapping code point ranges with an 'Int' payload, looked up
--   by binary search. The data comes from "Tables", which is generated.
module System.Console.Rustyline.Internal.Unicode.RangeTable
  ( RangeTable,
    mkRangeTable,
    lookupRange,
    inRange,
  )
where

import Data.Array.Base (unsafeAt)
import Data.Array.Unboxed (UArray, listArray)

-- | Length, lower bounds, upper bounds and payloads.
data RangeTable
  = RangeTable
      !Int
      !(UArray Int Int)
      !(UArray Int Int)
      !(UArray Int Int)

-- | Build a table from parallel lists of inclusive lower bounds, upper
--   bounds and values. The ranges must be sorted and disjoint.
mkRangeTable :: [Int] -> [Int] -> [Int] -> RangeTable
mkRangeTable los his vals =
  RangeTable n (arr los) (arr his) (arr vals)
  where
    n = length los
    arr :: [Int] -> UArray Int Int
    arr = listArray (0, n - 1)

-- | The payload of the range containing the code point, or the default.
lookupRange :: RangeTable -> Int -> Int -> Int
lookupRange (RangeTable n los his vals) def cp = go 0 (n - 1)
  where
    -- invariant: every range before @lo@ ends below @cp@, every range after
    -- @hi@ starts above it
    go lo hi
      | lo > hi = def
      | otherwise =
          let mid = (lo + hi) `div` 2
           in if cp < unsafeAt los mid
                then go lo (mid - 1)
                else
                  if cp > unsafeAt his mid
                    then go (mid + 1) hi
                    else unsafeAt vals mid

-- | Whether the code point lies in any range of the table.
inRange :: RangeTable -> Int -> Bool
inRange t cp = lookupRange t 0 cp /= 0
