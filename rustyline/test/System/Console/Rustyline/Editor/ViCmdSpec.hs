-- | Ported from rustyline's @src/test/vi_cmd.rs@.
module System.Console.Rustyline.Editor.ViCmdSpec (spec) where

import Control.Monad (forM_)
import System.Console.Rustyline
import System.Console.Rustyline.Editor.Harness
import Test.Hspec

spec :: Spec
spec = do
  it "dollar" $ do
    assertCursor Vi ("", "Hi") [ESC, fromChar '$', ENTER] ("Hi", "")

  it "dot" $ do
    assertCursor Vi ("", "") [ESC, fromChar '.', ENTER] ("", "")

  it "semi_colon" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 'f', fromChar 'o', fromChar ';', ENTER] ("Hello, w", "orld!")

  it "comma" $ do
    assertCursor Vi ("Hello, w", "orld!") [ESC, fromChar 'f', fromChar 'l', fromChar ',', ENTER] ("Hel", "lo, world!")

  it "zero" $ do
    assertCursor Vi ("Hi", "") [ESC, fromChar '0', ENTER] ("", "Hi")

  it "caret" $ do
    assertCursor Vi (" Hi", "") [ESC, fromChar '^', ENTER] (" ", "Hi")

  it "caret_no_whitespace" $ do
    assertCursor Vi ("Hi", "") [ESC, fromChar '^', ENTER] ("", "Hi")

  it "a" $ do
    assertCursor Vi ("B", "e") [ESC, fromChar 'a', fromChar 'y', ENTER] ("By", "e")

  it "uppercase_a" $ do
    assertCursor Vi ("", "By") [ESC, fromChar 'A', fromChar 'e', ENTER] ("Bye", "")

  it "b" $ do
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar 'b', ENTER] ("Hello, ", "world!")
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar '2', fromChar 'b', ENTER] ("Hello", ", world!")

  it "uppercase_b" $ do
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar 'B', ENTER] ("Hello, ", "world!")
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar '2', fromChar 'B', ENTER] ("", "Hello, world!")

  it "uppercase_c" $ do
    assertCursor Vi ("Hello, w", "orld!") [ESC, fromChar 'C', fromChar 'i', ENTER] ("Hello, i", "")

  it "ctrl_k" $ do
    forM_ [fromChar 'D', ctrl 'K'] $ \key -> do
      assertCursor Vi ("Hi", "") [ESC, key, ENTER] ("H", "")
      assertCursor Vi ("", "Hi") [ESC, key, ENTER] ("", "")
      assertCursor Vi ("By", "e") [ESC, key, ENTER] ("B", "")

  it "e" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 'e', ENTER] ("Hell", "o, world!")
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar '2', fromChar 'e', ENTER] ("Hello, worl", "d!")

  it "uppercase_e" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 'E', ENTER] ("Hello", ", world!")
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar '2', fromChar 'E', ENTER] ("Hello, world", "!")

  it "f" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 'f', fromChar 'r', ENTER] ("Hello, wo", "rld!")
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar '3', fromChar 'f', fromChar 'l', ENTER] ("Hello, wor", "ld!")

  it "uppercase_f" $ do
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar 'F', fromChar 'r', ENTER] ("Hello, wo", "rld!")
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar '3', fromChar 'F', fromChar 'l', ENTER] ("He", "llo, world!")

  it "i" $ do
    assertCursor Vi ("Be", "") [ESC, fromChar 'i', fromChar 'y', ENTER] ("By", "e")

  it "uppercase_i" $ do
    assertCursor Vi ("Be", "") [ESC, fromChar 'I', fromChar 'y', ENTER] ("y", "Be")

  it "u" $ do
    assertCursor Vi ("Hello, ", "world") [ESC, ctrl 'W', fromChar 'u', ENTER] ("Hello,", " world")

  it "w" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 'w', ENTER] ("Hello", ", world!")
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar '2', fromChar 'w', ENTER] ("Hello, ", "world!")

  it "uppercase_w" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 'W', ENTER] ("Hello, ", "world!")
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar '2', fromChar 'W', ENTER] ("Hello, world", "!")

  it "x" $ do
    assertCursor Vi ("", "a") [ESC, fromChar 'x', ENTER] ("", "")

  it "uppercase_x" $ do
    assertCursor Vi ("Hi", "") [ESC, fromChar 'X', ENTER] ("", "i")

  it "h" $ do
    forM_ [fromChar 'h', ctrl 'H', BACKSPACE] $ \key -> do
      assertCursor Vi ("Bye", "") [ESC, key, ENTER] ("B", "ye")
      assertCursor Vi ("Bye", "") [ESC, fromChar '2', key, ENTER] ("", "Bye")

  it "l" $ do
    forM_ [fromChar 'l', fromChar (' ')] $ \key -> do
      assertCursor Vi ("", "Hi") [ESC, key, ENTER] ("H", "i")
      assertCursor Vi ("", "Hi") [ESC, fromChar '2', key, ENTER] ("Hi", "")

  it "j" $ do
    forM_ [fromChar 'j', fromChar '+'] $ \key -> do
      assertCursor Vi ("Hel", "lo,\nworld!") [ESC, key, ENTER] ("Hello,\nwo", "rld!")
      assertCursor Vi ("", "One\nTwo\nThree") [ESC, fromChar '2', key, ENTER] ("One\nTwo\n", "Three")
      assertCursor Vi ("Hel", "lo,\nworld!") [ESC, fromChar '7', key, ENTER] ("Hello,\nwo", "rld!")

  it "k" $ do
    forM_ [fromChar 'k', fromChar '-'] $ \key -> do
      assertCursor Vi ("Hello,\nworl", "d!") [ESC, key, ENTER] ("Hel", "lo,\nworld!")
      assertCursor Vi ("One\nTwo\nT", "hree") [ESC, fromChar '2', key, ENTER] ("", "One\nTwo\nThree")
      assertCursor Vi ("Hello,\nworl", "d!") [ESC, fromChar '5', key, ENTER] ("Hel", "lo,\nworld!")
      assertCursor Vi ("first line\nshort\nlong line", "") [ESC, key, ENTER] ("first line\nshort", "\nlong line")

  it "ctrl_n" $ do
    forM_ [ctrl 'N'] $ \key -> do
      assertHistory Vi ["line1", "line2"] [ESC, ctrl 'P', ctrl 'P', key, ENTER] "" ("line2", "")

  it "ctrl_p" $ do
    forM_ [ctrl 'P'] $ \key -> do
      assertHistory Vi ["line1"] [ESC, key, ENTER] "" ("line1", "")

  it "p" $ do
    assertCursor Vi ("Hello, ", "world") [ESC, ctrl 'W', fromChar 'p', ENTER] (" Hello", ",world")

  it "uppercase_p" $ do
    assertCursor Vi ("Hello, ", "world") [ESC, ctrl 'W', fromChar 'P', ENTER] ("Hello", ", world")

  it "r" $ do
    assertCursor Vi ("Hi", ", world!") [ESC, fromChar 'r', fromChar 'o', ENTER] ("H", "o, world!")
    assertCursor Vi ("He", "llo, world!") [ESC, fromChar '4', fromChar 'r', fromChar 'i', ENTER] ("Hiii", "i, world!")

  it "s" $ do
    assertCursor Vi ("Hi", ", world!") [ESC, fromChar 's', fromChar 'o', ENTER] ("Ho", ", world!")
    assertCursor Vi ("He", "llo, world!") [ESC, fromChar '4', fromChar 's', fromChar 'i', ENTER] ("Hi", ", world!")

  it "uppercase_s" $ do
    assertCursor Vi ("Hello, ", "world") [ESC, fromChar 'S', ENTER] ("", "")

  it "t" $ do
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar 't', fromChar 'r', ENTER] ("Hello, w", "orld!")
    assertCursor Vi ("", "Hello, world!") [ESC, fromChar '3', fromChar 't', fromChar 'l', ENTER] ("Hello, wo", "rld!")

  it "uppercase_t" $ do
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar 'T', fromChar 'r', ENTER] ("Hello, wor", "ld!")
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar '3', fromChar 'T', fromChar 'l', ENTER] ("Hel", "lo, world!")

  it "indent" $ do
    assertCursor Vi ("Hello, world!", "") [ESC, fromChar '>', fromChar '>', ENTER] ("  Hello, world", "!")
    assertCursor Vi ("line1\nline2", "") [ESC, fromChar '>', fromChar '>', ENTER] ("line1\n  line", "2")
    assertCursor Vi ("line1\nline2", "") [ESC, fromChar '>', fromChar 'k', ENTER] ("  line1\n  line", "2")
    assertCursor Vi ("  li", "ne1\n  line2") [ESC, fromChar '>', fromChar 'j', ENTER] ("    l", "ine1\n    line2")
    assertCursor Vi ("  ", "line1\n  line2") [ESC, fromChar '>', fromChar 'j', ENTER] ("   ", " line1\n    line2")
    assertCursor Vi ("  ", "line1\n  line2") [ESC, fromChar '>', fromChar 'j', ENTER] ("   ", " line1\n    line2")

  it "dedent" $ do
    assertCursor Vi ("  line1\n  line2", "") [ESC, fromChar '<', fromChar '<', ENTER] ("  line1\nline", "2")
    assertCursor Vi ("  line1\n  line2", "") [ESC, fromChar '<', fromChar 'k', ENTER] ("line1\nline", "2")
    assertCursor Vi ("  li", "ne1\n  line2") [ESC, fromChar '<', fromChar 'j', ENTER] ("l", "ine1\nline2")
    assertCursor Vi ("  ", "line1\n  line2") [ESC, fromChar '<', fromChar 'j', ENTER] ("", "line1\nline2")
    assertCursor Vi ("line", "1\n  line2") [ESC, fromChar '<', fromChar 'j', ENTER] ("lin", "e1\nline2")
