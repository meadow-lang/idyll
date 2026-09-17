module System.Console.Rustyline.KillRingSpec (spec) where

import Data.Foldable (toList)
import Data.Text (Text)
import System.Console.Rustyline.KillRing
import Test.Hspec

killReset :: [Text] -> KillRing -> KillRing
killReset ws kr0 = foldl step kr0 (zip [0 :: Int ..] ws)
  where
    step kr (i, w) = killText w Append (if i == 0 then kr else resetKillRing kr)

spec :: Spec
spec = do
  it "disabled" $ do
    let kr = killText "text" Append (newKillRing 0)
    toList (krSlots kr) `shouldBe` []
    krIndex kr `shouldBe` 0
    krLastAction kr `shouldBe` KAKill
    let (y, kr') = yankText kr
    y `shouldBe` Nothing
    krLastAction kr' `shouldBe` KAKill

  it "one_kill" $ do
    let kr = killText "word1" Append (newKillRing 2)
    krIndex kr `shouldBe` 0
    toList (krSlots kr) `shouldBe` ["word1"]
    krLastAction kr `shouldBe` KAKill

  it "kill_append" $ do
    let kr = killText " word2" Append (killText "word1" Append (newKillRing 2))
    krIndex kr `shouldBe` 0
    toList (krSlots kr) `shouldBe` ["word1 word2"]
    krLastAction kr `shouldBe` KAKill

  it "kill_backward" $ do
    let kr = killText "word2 " Prepend (killText "word1" Prepend (newKillRing 2))
    krIndex kr `shouldBe` 0
    toList (krSlots kr) `shouldBe` ["word2 word1"]
    krLastAction kr `shouldBe` KAKill

  it "kill_other_kill" $ do
    let kr = killReset ["word1", "word2"] (newKillRing 2)
    krIndex kr `shouldBe` 1
    toList (krSlots kr) `shouldBe` ["word1", "word2"]
    krLastAction kr `shouldBe` KAKill

  it "many_kill" $ do
    let kr = killReset ["word1", "word2", "word3", "word4"] (newKillRing 2)
    krIndex kr `shouldBe` 1
    toList (krSlots kr) `shouldBe` ["word3", "word4"]
    krLastAction kr `shouldBe` KAKill

  it "yank" $ do
    let kr = killReset ["word1", "word2"] (newKillRing 2)
        (y1, kr1) = yankText kr
        (y2, kr2) = yankText kr1
    y1 `shouldBe` Just "word2"
    krLastAction kr1 `shouldBe` KAYank 5
    y2 `shouldBe` Just "word2"
    krLastAction kr2 `shouldBe` KAYank 5

  it "yank_pop" $ do
    let kr = killReset ["word1", "longword2"] (newKillRing 2)
        (p0, kr0) = yankPopText kr
        (_, kr1) = yankText kr0
        (p1, kr2) = yankPopText kr1
        (p2, kr3) = yankPopText kr2
        (p3, _) = yankPopText kr3
    p0 `shouldBe` Nothing
    p1 `shouldBe` Just (9, "word1")
    p2 `shouldBe` Just (5, "longword2")
    p3 `shouldBe` Just (9, "word1")
