module System.Console.Rustyline.UndoSpec (spec) where

import System.Console.Rustyline.LineBuffer
import System.Console.Rustyline.Undo
import Test.Hspec

buffer :: LB NoListener a -> LineBuffer
buffer m = snd (evalLB NoListener (initLineBuffer "" 0) m)

spec :: Spec
spec = do
  it "test_insert_chars" $ do
    let cs = insertChange 1 'i' (insertChange 0 'H' newChangeset)
    undoCount cs `shouldBe` 1
    redoCount cs `shouldBe` 0
    undoCount (insertChange 0 ' ' cs) `shouldBe` 2

  it "test_insert_strings" $ do
    let cs = insertStrChange 5 ", " (insertStrChange 0 "Hello" newChangeset)
    undoCount cs `shouldBe` 2
    redoCount cs `shouldBe` 0

  it "test_undo_insert" $ do
    let buf = buffer (insertStr 0 "Hello" >> insertStr 5 ", world!")
    asStr buf `shouldBe` "Hello, world!"
    let cs = insertStrChange 5 ", world!" newChangeset
        (_, cs1, buf1) = undo 1 cs buf
    undoCount cs1 `shouldBe` 0
    redoCount cs1 `shouldBe` 1
    asStr buf1 `shouldBe` "Hello"
    let (_, cs2, buf2) = redo cs1 buf1
    undoCount cs2 `shouldBe` 1
    redoCount cs2 `shouldBe` 0
    asStr buf2 `shouldBe` "Hello, world!"

  it "test_undo_delete" $ do
    let buf = buffer (insertStr 0 "Hello")
        cs = deleteChange 5 ", world!" newChangeset
        (_, cs1, buf1) = undo 1 cs buf
    asStr buf1 `shouldBe` "Hello, world!"
    let (_, _, buf2) = redo cs1 buf1
    asStr buf2 `shouldBe` "Hello"

  it "test_delete_chars" $ do
    let buf = buffer (insertStr 0 "Hlo")
        cs = deleteChange 1 "l" (deleteChange 1 "e" newChangeset)
    undoCount cs `shouldBe` 1
    let (_, _, buf1) = undo 1 cs buf
    asStr buf1 `shouldBe` "Hello"

  it "test_backspace_chars" $ do
    let buf = buffer (insertStr 0 "Hlo")
        cs = deleteChange 1 "e" (deleteChange 2 "l" newChangeset)
    undoCount cs `shouldBe` 1
    let (_, _, buf1) = undo 1 cs buf
    asStr buf1 `shouldBe` "Hello"

  it "test_undo_replace" $ do
    let buf = buffer (insertStr 0 "Hello, world!" >> replace 1 5 "i")
    asStr buf `shouldBe` "Hi, world!"
    let cs = replaceChange 1 "ello" "i" newChangeset
        (_, cs1, buf1) = undo 1 cs buf
    asStr buf1 `shouldBe` "Hello, world!"
    let (_, _, buf2) = redo cs1 buf1
    asStr buf2 `shouldBe` "Hi, world!"

  it "test_last_insert" $ do
    let (_, cs0) = begin newChangeset
        cs1 = insertStrChange 0 "Bye" (deleteChange 0 "Hello" cs0)
        (_, cs2) = end cs1
    lastInsert cs2 `shouldBe` Just "Bye"

  it "test_end" $ do
    let (_, cs0) = begin newChangeset
        (touched0, cs1) = end cs0
    touched0 `shouldBe` False
    let (_, cs2) = begin cs1
        (touched1, _) = end (insertStrChange 0 "Hi" cs2)
    touched1 `shouldBe` True
