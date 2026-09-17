-- | Ported from rustyline's @src/test/emacs.rs@.
module System.Console.Rustyline.Editor.EmacsSpec (spec) where

import System.Console.Rustyline
import System.Console.Rustyline.Editor.Harness
import Test.Hspec

spec :: Spec
spec = do
  it "ctrl_a" $ do
    assertCursor Emacs ("Hi", "") [ctrl 'A', ENTER] ("", "Hi")
    assertCursor Emacs ("test test\n123", "foo") [ctrl 'A', ENTER] ("test test\n", "123foo")

  it "ctrl_e" $ do
    assertCursor Emacs ("", "Hi") [ctrl 'E', ENTER] ("Hi", "")
    assertCursor Emacs ("foo", "test test\n123") [ctrl 'E', ENTER] ("footest test", "\n123")

  it "ctrl_b" $ do
    assertCursor Emacs ("Hi", "") [ctrl 'B', ENTER] ("H", "i")
    assertCursor Emacs ("Hi", "") [alt '2', ctrl 'B', ENTER] ("", "Hi")
    assertCursor Emacs ("", "Hi") [alt '-', alt '2', ctrl 'B', ENTER] ("Hi", "")

  it "ctrl_f" $ do
    assertCursor Emacs ("", "Hi") [ctrl 'F', ENTER] ("H", "i")
    assertCursor Emacs ("", "Hi") [alt '2', ctrl 'F', ENTER] ("Hi", "")
    assertCursor Emacs ("Hi", "") [alt '-', alt '2', ctrl 'F', ENTER] ("", "Hi")

  it "ctrl_h" $ do
    assertCursor Emacs ("Hi", "") [ctrl 'H', ENTER] ("H", "")
    assertCursor Emacs ("Hi", "") [alt '2', ctrl 'H', ENTER] ("", "")
    assertCursor Emacs ("", "Hi") [alt '-', alt '2', ctrl 'H', ENTER] ("", "")

  it "backspace" $ do
    assertCursor Emacs ("", "") [BACKSPACE, ENTER] ("", "")
    assertCursor Emacs ("Hi", "") [BACKSPACE, ENTER] ("H", "")
    assertCursor Emacs ("", "Hi") [BACKSPACE, ENTER] ("", "Hi")

  it "ctrl_k" $ do
    assertCursor Emacs ("Hi", "") [ctrl 'K', ENTER] ("Hi", "")
    assertCursor Emacs ("", "Hi") [ctrl 'K', ENTER] ("", "")
    assertCursor Emacs ("B", "ye") [ctrl 'K', ENTER] ("B", "")
    assertCursor Emacs ("Hi", "foo\nbar") [ctrl 'K', ENTER] ("Hi", "\nbar")
    assertCursor Emacs ("Hi", "\nbar") [ctrl 'K', ENTER] ("Hi", "bar")
    assertCursor Emacs ("Hi", "bar") [ctrl 'K', ENTER] ("Hi", "")

  it "ctrl_u" $ do
    assertCursor Emacs ("", "Hi") [ctrl 'U', ENTER] ("", "Hi")
    assertCursor Emacs ("Hi", "") [ctrl 'U', ENTER] ("", "")
    assertCursor Emacs ("B", "ye") [ctrl 'U', ENTER] ("", "ye")
    assertCursor Emacs ("foo\nbar", "Hi") [ctrl 'U', ENTER] ("foo\n", "Hi")
    assertCursor Emacs ("foo\n", "Hi") [ctrl 'U', ENTER] ("foo", "Hi")
    assertCursor Emacs ("foo", "Hi") [ctrl 'U', ENTER] ("", "Hi")

  it "ctrl_n" $ do
    assertHistory Emacs ["line1", "line2"] [ctrl 'P', ctrl 'P', ctrl 'N', ENTER] "" ("line2", "")

  it "ctrl_p" $ do
    assertHistory Emacs ["line1"] [ctrl 'P', ENTER] "" ("line1", "")

  it "ctrl_t" $ do
    pendingWith "FIXME in rustyline"
    {- FIXME
    assert_cursor(
        ("ab", "cd"),
        &[E::alt('2'), E::ctrl('T'), E::ENTER],
        ("acdb", ""),
    );-}

  it "ctrl_x_ctrl_u" $ do
    assertCursor Emacs ("Hello, ", "world") [ctrl 'W', ctrl 'X', ctrl 'U', ENTER] ("Hello, ", "world")

  it "ctrl_x_ctrl_x" $ do
    editor <- initEditor Emacs [ctrl 'X', ctrl 'X', ENTER]
    _ <- bindSequence editor (KeySeq [ctrl 'X', ctrl 'E']) (Simple Noop)
    (either (Left . show) Right <$> readline editor "") `shouldReturn` Right ""

  it "meta_b" $ do
    assertCursor Emacs ("Hello, world!", "") [alt 'B', ENTER] ("Hello, ", "world!")
    assertCursor Emacs ("Hello, world!", "") [alt '2', alt 'B', ENTER] ("", "Hello, world!")
    assertCursor Emacs ("", "Hello, world!") [alt '-', alt 'B', ENTER] ("Hello", ", world!")

  it "meta_f" $ do
    assertCursor Emacs ("", "Hello, world!") [alt 'F', ENTER] ("Hello", ", world!")
    assertCursor Emacs ("", "Hello, world!") [alt '2', alt 'F', ENTER] ("Hello, world", "!")
    assertCursor Emacs ("Hello, world!", "") [alt '-', alt 'F', ENTER] ("Hello, ", "world!")

  it "meta_c" $ do
    assertCursor Emacs ("hi", "") [alt 'C', ENTER] ("hi", "")
    assertCursor Emacs ("", "hi") [alt 'C', ENTER] ("Hi", "")
    {- FIXME
    assert_cursor(
        ("", "hi test"),
        &[E::alt('2'), E::alt('C'), E::ENTER],
        ("Hi Test", ""),
    );-}

  it "meta_l" $ do
    assertCursor Emacs ("Hi", "") [alt 'L', ENTER] ("Hi", "")
    assertCursor Emacs ("", "HI") [alt 'L', ENTER] ("hi", "")
    {- FIXME
    assert_cursor(
        ("", "HI TEST"),
        &[E::alt('2'), E::alt('L'), E::ENTER],
        ("hi test", ""),
    );-}

  it "meta_u" $ do
    assertCursor Emacs ("hi", "") [alt 'U', ENTER] ("hi", "")
    assertCursor Emacs ("", "hi") [alt 'U', ENTER] ("HI", "")
    {- FIXME
    assert_cursor(
        ("", "hi test"),
        &[E::alt('2'), E::alt('U'), E::ENTER],
        ("HI TEST", ""),
    );-}

  it "meta_d" $ do
    assertCursor Emacs ("Hello", ", world!") [alt 'D', ENTER] ("Hello", "!")
    assertCursor Emacs ("Hello", ", world!") [alt '2', alt 'D', ENTER] ("Hello", "")

  it "meta_t" $ do
    assertCursor Emacs ("Hello", ", world!") [alt 'T', ENTER] ("world, Hello", "!")
    {- FIXME
    assert_cursor(
        ("One Two", " Three Four"),
        &[E::alt('T'), E::ENTER],
        ("One Four Three Two", ""),
    );-}

  it "meta_y" $ do
    assertCursor Emacs ("Hello, world", "!") [ctrl 'W', KeyEvent KLeft NONE, ctrl 'W', ctrl 'Y', alt 'Y', ENTER] ("world", " !")

  it "meta_backspace" $ do
    assertCursor Emacs ("Hello, wor", "ld!") [KeyEvent KBackspace ALT, ENTER] ("Hello, ", "ld!")

  it "meta_digit" $ do
    assertCursor Emacs ("", "") [alt '3', fromChar 'h', ENTER] ("hhh", "")
