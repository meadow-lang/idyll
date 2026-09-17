-- | Unix terminal (@tty/unix.rs@).
--
--   Differences from Rust worth knowing:
--
--   * Waiting for input goes through a @select(2)@ shim so that the signal
--     self-pipe and the external-printer pipe can be watched together with
--     the terminal. On the non-threaded RTS the wait is sliced so Haskell
--     signal handlers get a chance to run.
--   * Output is written through a 'Handle' (stdout, or @/dev/tty@) so it
--     interleaves correctly with the application's own buffered output.
--   * Typeahead left in the read buffer is handed back by 'unbuffer' (the
--     @buffer-redux@ behaviour).
module System.Console.Rustyline.Tty.Unix
  ( newPosixTerminal,
    getWinSize,
  )
where

import Control.Concurrent (MVar, newEmptyMVar, putMVar, rtsSupportsBoundThreads, tryTakeMVar, yield)
import Control.Exception (IOException, SomeException, throwIO, try)
import Control.Monad (unless, void, when)
import Data.Bits (shiftL, testBit, (.&.), (.|.))
import Data.ByteString (ByteString)
import Data.ByteString qualified as BS
import Data.Char (chr, isDigit, ord)
import Data.IORef
import Data.Map.Strict qualified as Map
import Data.Maybe (catMaybes, fromMaybe, isJust)
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Encoding qualified as TE
import Data.Word (Word8)
import Foreign.C.Error (eINTR, eNOTTY, errnoToIOError, getErrno, throwErrno)
import Foreign.C.Types (CInt (..))
import Foreign.Marshal.Alloc (alloca, allocaBytes)
import Foreign.Ptr (Ptr, castPtr)
import Foreign.Storable (peek, poke)
import GHC.Clock (getMonotonicTimeNSec)
import System.Console.Rustyline.Config
import System.Console.Rustyline.Error
import System.Console.Rustyline.Highlight (Highlighter (..))
import System.Console.Rustyline.Internal.Unicode.Grapheme (graphemes)
import System.Console.Rustyline.Keymap.Types (Cmd (..))
import System.Console.Rustyline.Keys
import System.Console.Rustyline.Layout
import System.Console.Rustyline.LineBuffer (asStr, pos)
import System.Console.Rustyline.Prompt
import System.Console.Rustyline.Tty
import System.IO (Handle, hFlush, stdout)
import System.Posix.IO (OpenMode (..), closeFd, createPipe, defaultFileFlags, fdReadBuf, fdToHandle, fdWriteBuf, openFd, setFdOption, FdOption (..), dup)
import System.Posix.Signals (Handler (..), installHandler, sigINT, sigTSTP, signalProcess)
import System.Posix.Signals.Exts (windowChange)
import System.Posix.Terminal (TerminalAttributes, TerminalMode (..), TerminalState (..), controlChar, getTerminalAttributes, queryTerminal, setTerminalAttributes, withBits, withMinInput, withMode, withTime, withoutMode)
import System.Posix.Terminal qualified as PT
import System.Posix.Types (Fd (..))

foreign import ccall safe "rustyline_wait_readable"
  c_waitReadable :: CInt -> CInt -> CInt -> CInt -> IO CInt

foreign import ccall unsafe "rustyline_get_winsize"
  c_getWinSize :: CInt -> Ptr CInt -> Ptr CInt -> IO CInt

bracketedPasteOn, bracketedPasteOff, beginSynchronizedUpdateSeq, endSynchronizedUpdateSeq :: Text
bracketedPasteOn = "\x1b[?2004h"
bracketedPasteOff = "\x1b[?2004l"
beginSynchronizedUpdateSeq = "\x1b[?2026h"
endSynchronizedUpdateSeq = "\x1b[?2026l"

unitMax :: Unit
unitMax = 65535

-- | Columns and rows of the terminal, (80, 24) when unknown.
getWinSize :: Fd -> IO (Unit, Unit)
getWinSize (Fd fd) =
  alloca $ \pc -> alloca $ \pr -> do
    r <- c_getWinSize fd pc pr
    if r /= 0
      then pure (80, 24)
      else do
        c <- fromIntegral <$> peek pc
        rw <- fromIntegral <$> peek pr
        -- In linux pseudo-terminals are created with dimensions of zero. If
        -- the host application didn't initialize the size, treat zero as 80
        -- columns and infinite rows.
        pure (if c == 0 then 80 else c, if rw == 0 then unitMax else rw)

-- | Wait until one of the descriptors is readable. Returns the readiness
--   bitmask (0 on timeout). A negative timeout waits forever. EINTR is
--   retried: signals are reported through the self-pipe.
waitReadable :: [Maybe Fd] -> Int -> IO Int
waitReadable fds timeoutMs = do
  start <- getMonotonicTimeNSec
  let raw i = case drop i fds of
        (Just (Fd x) : _) -> x
        _ -> -1
      (a, b, c) = (raw 0, raw 1, raw 2)
      -- without bound threads a blocking foreign call stops every Haskell
      -- thread, including signal handlers: wait in short slices instead
      slice = if rtsSupportsBoundThreads then Nothing else Just 50
      loop = do
        now <- getMonotonicTimeNSec
        let elapsed = fromIntegral ((now - start) `div` 1000000)
            remaining = if timeoutMs < 0 then -1 else max 0 (timeoutMs - elapsed)
            wait = case slice of
              Just s | remaining < 0 -> s
              Just s -> min s remaining
              Nothing -> remaining
        r <- c_waitReadable a b c (fromIntegral wait)
        if
          | r < 0 -> do
              errno <- getErrno
              if errno == eINTR then yield >> loop else throwErrno "select"
          | r > 0 -> pure (fromIntegral r)
          | timeoutMs >= 0 && wait >= remaining -> pure 0
          | otherwise -> yield >> loop
  loop

writeByte :: Fd -> Word8 -> IO ()
writeByte fd w = void . try @IOException $ alloca $ \p -> poke p w >> fdWriteBuf fd p 1

-- | Read up to @n@ bytes; 'Nothing' when nothing is available on a
--   non-blocking descriptor.
readSome :: Fd -> Int -> IO (Maybe ByteString)
readSome fd n = allocaBytes n $ \p -> do
  r <- try @IOException (fdReadBuf fd p (fromIntegral n))
  case r of
    Left _ -> pure Nothing
    Right k -> Just <$> BS.packCStringLen (castPtr p, fromIntegral k)

-- | SIGINT / SIGWINCH self-pipe.
data Sig = Sig
  { sigPipe :: Fd,
    sigUninstall :: IO ()
  }

installSigHandlers :: IO Sig
installSigHandlers = do
  (r, w) <- createPipe
  setFdOption r NonBlockingRead True
  setFdOption w NonBlockingRead True
  oldInt <- installHandler sigINT (Catch (writeByte w (fromIntegral (ord 'I')))) Nothing
  oldWinch <- installHandler windowChange (Catch (writeByte w (fromIntegral (ord 'W')))) Nothing
  pure
    Sig
      { sigPipe = r,
        sigUninstall = do
          _ <- installHandler sigINT oldInt Nothing
          _ <- installHandler windowChange oldWinch Nothing
          closeFd r
          closeFd w
      }

-- | Check if a signal has been received.
readSignal :: Sig -> IO (Maybe Signal)
readSignal sig =
  readSome (sigPipe sig) 64 >>= \case
    Just bytes | not (BS.null bytes) -> pure $ case BS.head bytes of
      73 -> Just SigInterrupt -- 'I'
      _ -> Just SigResize -- 'W'
    _ -> pure Nothing

-- | External print pipe: readable descriptor plus the message slot.
data PipeReader = PipeReader Fd (MVar Text)

data PipeWriter = PipeWriter Fd (MVar Text)

invalidData :: IO a
invalidData = throwIO (ReadlineIo (userError "stream did not contain valid UTF-8"))

newPosixReader :: Fd -> Maybe Sig -> Maybe Buffer -> Config -> KeyMap -> Maybe PipeReader -> IO RawReader
newPosixReader tty sig initial config keyMap pipeReader = do
  bufRef <- newIORef (fromMaybe BS.empty initial)
  let timeoutMs :: Int
      timeoutMs = maybe (-1) fromIntegral (keyseqTimeout config)
      sigFd = sigPipe <$> sig

      checkSignal = case sig of
        Just s -> readSignal s >>= maybe (pure ()) (throwIO . ReadlineSignal)
        Nothing -> pure ()

      -- Blocking read into the buffer; False on end of file.
      fill = do
        mask <- waitReadable [Just tty, sigFd] (-1)
        when (testBit mask 1) checkSignal
        if testBit mask 0
          then
            readSome tty 1024 >>= \case
              Nothing -> fill -- spurious wakeup
              Just bytes
                | BS.null bytes -> pure False
                | otherwise -> True <$ writeIORef bufRef bytes
          else fill

      nextByte = do
        buf <- readIORef bufRef
        case BS.uncons buf of
          Just (b, rest) -> b <$ writeIORef bufRef rest
          Nothing -> do
            ok <- fill
            if ok then nextByte else throwIO Eof

      nextChar' = do
        b0 <- nextByte
        let cont acc 0 = pure acc
            cont acc k = do
              b <- nextByte
              if b .&. 0xC0 /= 0x80
                then invalidData
                else cont ((acc `shiftL` 6) .|. fromIntegral (b .&. 0x3F)) (k - 1 :: Int)
            decode lead k = do
              cp <- cont (fromIntegral lead) k
              if cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF) then invalidData else pure (chr cp)
        if
          | b0 < 0x80 -> pure (chr (fromIntegral b0))
          | b0 >= 0xC2 && b0 <= 0xDF -> decode (b0 .&. 0x1F) 1
          | b0 >= 0xE0 && b0 <= 0xEF -> decode (b0 .&. 0x0F) 2
          | b0 >= 0xF0 && b0 <= 0xF4 -> decode (b0 .&. 0x07) 3
          | otherwise -> invalidData

      poll' timeout = do
        buf <- readIORef bufRef
        if not (BS.null buf)
          then pure True
          else do
            mask <- waitReadable [Just tty, sigFd] timeout
            when (testBit mask 1) checkSignal
            pure (testBit mask 0)

      -- Handle \E <seq1> sequences
      escapeSequence = doEscapeSequence True
      doEscapeSequence allowRecurse = do
        seq1 <- nextChar'
        if
          | seq1 == '[' -> escapeCsi -- \E[ sequences. (CSI)
          | seq1 == 'O' -> escapeO -- \EO sequences. (SS3)
          | seq1 == '\x1b' ->
              -- \E\E: rxvt / iTerm Alt+key. ESC ESC with nothing following
              -- is the escape key.
              if not allowRecurse
                then pure ESC
                else do
                  let timeout = if timeoutMs < 0 then 100 else timeoutMs
                  -- ignore poll errors, the next read will very likely report them
                  ready <- either (\(_ :: SomeException) -> False) id <$> try (poll' timeout)
                  if ready
                    then do
                      KeyEvent k m <- doEscapeSequence False
                      pure (KeyEvent k (m <> ALT))
                    else pure ESC
          | otherwise -> pure (alt seq1)

      unknown = KeyEvent KUnknownEscSeq NONE
      plain k = KeyEvent k NONE

      -- Handle \E[ <seq2> escape sequences
      escapeCsi = do
        seq2 <- nextChar'
        if
          | isDigit seq2 -> if seq2 == '0' || seq2 == '9' then pure unknown else extendedEscape seq2
          | seq2 == '[' -> do
              -- Linux console
              seq3 <- nextChar'
              pure $ case seq3 of
                'A' -> plain (KF 1)
                'B' -> plain (KF 2)
                'C' -> plain (KF 3)
                'D' -> plain (KF 4)
                'E' -> plain (KF 5)
                _ -> unknown
          | otherwise -> pure $ case seq2 of
              -- ANSI
              'A' -> plain KUp
              'B' -> plain KDown
              'C' -> plain KRight
              'D' -> plain KLeft
              'F' -> plain KEnd
              'H' -> plain KHome
              'Z' -> plain KBackTab
              'a' -> KeyEvent KUp SHIFT -- rxvt: kind or kUP
              'b' -> KeyEvent KDown SHIFT -- rxvt: kri or kDN
              'c' -> KeyEvent KRight SHIFT -- rxvt
              'd' -> KeyEvent KLeft SHIFT -- rxvt
              _ -> unknown

      -- Handle \E[ <seq2:digit> escape sequences
      extendedEscape seq2 = do
        seq3 <- nextChar'
        if
          | seq3 == '~' -> pure $ case seq2 of
              '1' -> plain KHome -- tmux
              '7' -> plain KHome -- rxvt
              '2' -> plain KInsert
              '3' -> plain KDelete
              '4' -> plain KEnd -- tmux
              '8' -> plain KEnd -- rxvt
              '5' -> plain KPageUp
              '6' -> plain KPageDown
              _ -> unknown
          | isDigit seq3 -> do
              seq4 <- nextChar'
              if
                | seq4 == '~' -> pure $ maybe unknown plain (lookup [seq2, seq3] fkeys)
                | seq4 == ';' -> do
                    seq5 <- nextChar'
                    if isDigit seq5
                      then do
                        seq6 <- nextChar'
                        if
                          | isDigit seq6 -> unknown <$ nextChar' -- 'R' expected
                          | seq6 == 'R' -> pure unknown
                          | seq6 == '~' ->
                              pure $
                                if seq5 == '5'
                                  then maybe unknown (\k -> KeyEvent k CTRL) (lookup [seq2, seq3] ctrlFkeys)
                                  else unknown
                          | otherwise -> pure unknown
                      else pure unknown
                | isDigit seq4 -> do
                    seq5 <- nextChar'
                    pure $
                      if seq5 == '~'
                        then case [seq2, seq3, seq4] of
                          "200" -> plain KBracketedPasteStart
                          "201" -> plain KBracketedPasteEnd
                          _ -> unknown
                        else unknown
                | otherwise -> pure unknown
          | seq3 == ';' -> do
              seq4 <- nextChar'
              if isDigit seq4
                then do
                  seq5 <- nextChar'
                  if
                    | isDigit seq5 -> unknown <$ nextChar' -- 'R' expected
                    | seq2 == '1' -> pure (csiModified seq4 seq5)
                    | seq5 == '~' -> pure (tildeModified seq2 seq4)
                    | otherwise -> pure unknown
                else pure unknown
          | otherwise -> pure $ case (seq2, seq3) of
              ('3', '\x1e') -> KeyEvent KDelete CTRL
              ('3', '@') -> KeyEvent KDelete CTRL_SHIFT
              ('5', 'A') -> KeyEvent KUp CTRL
              ('5', 'B') -> KeyEvent KDown CTRL
              ('5', 'C') -> KeyEvent KRight CTRL
              ('5', 'D') -> KeyEvent KLeft CTRL
              ('5', '\x1e') -> KeyEvent KPageUp CTRL
              ('5', '$') -> KeyEvent KPageUp SHIFT
              ('5', '@') -> KeyEvent KPageUp CTRL_SHIFT
              ('6', '\x1e') -> KeyEvent KPageDown CTRL
              ('6', '$') -> KeyEvent KPageDown SHIFT
              ('6', '@') -> KeyEvent KPageDown CTRL_SHIFT
              ('7', '\x1e') -> KeyEvent KHome CTRL
              ('7', '$') -> KeyEvent KHome SHIFT
              ('7', '@') -> KeyEvent KHome CTRL_SHIFT
              ('8', '\x1e') -> KeyEvent KEnd CTRL -- kEND5 or kel
              ('8', '$') -> KeyEvent KEnd SHIFT
              ('8', '@') -> KeyEvent KEnd CTRL_SHIFT
              _ -> unknown

      -- Handle \EO <seq2> escape sequences
      escapeO = do
        seq2 <- nextChar'
        pure $ case seq2 of
          'A' -> plain KUp
          'B' -> plain KDown
          'C' -> plain KRight
          'D' -> plain KLeft
          'F' -> plain KEnd -- kend
          'H' -> plain KHome -- khome
          'M' -> ENTER -- kent
          'P' -> plain (KF 1) -- kf1
          'Q' -> plain (KF 2) -- kf2
          'R' -> plain (KF 3) -- kf3
          'S' -> plain (KF 4) -- kf4
          'a' -> KeyEvent KUp CTRL
          'b' -> KeyEvent KDown CTRL
          'c' -> KeyEvent KRight CTRL -- rxvt
          'd' -> KeyEvent KLeft CTRL -- rxvt
          'l' -> plain (KF 8)
          't' -> plain (KF 5) -- kf5 or kb1
          'u' -> plain (KF 6) -- kf6 or kb2
          'v' -> plain (KF 7) -- kf7 or kb3
          'w' -> plain (KF 9) -- kf9 or ka1
          'x' -> plain (KF 10) -- kf10 or ka2
          _ -> unknown

      nextKey' singleEscAbort = do
        c <- nextChar'
        let key = newKey c NONE
        if key == ESC
          then do
            let timeout = if singleEscAbort && timeoutMs < 0 then 0 else timeoutMs
            ready <- poll' timeout
            if ready then escapeSequence else pure key -- escape sequence / single escape
          else pure key

      select' singleEscAbort = do
        let pipeFd = (\(PipeReader fd _) -> fd) <$> pipeReader
            loop = do
              buffered <- not . BS.null <$> readIORef bufRef
              mask <- if buffered then pure 1 else waitReadable [Just tty, sigFd, pipeFd] (-1)
              if
                | testBit mask 1 -> do
                    checkSignal
                    loop
                | testBit mask 0 ->
                    -- prefer user input over external print
                    KeyPress <$> nextKey' singleEscAbort
                | Just (PipeReader fd slot) <- pipeReader, testBit mask 2 -> do
                    _ <- readSome fd 1
                    tryTakeMVar slot >>= maybe loop (pure . ExternalPrint)
                | otherwise -> loop
        loop

      readPasted = do
        let loop acc =
              nextChar' >>= \case
                '\x1b' -> do
                  key <- escapeSequence
                  if key == KeyEvent KBracketedPasteEnd NONE
                    then pure acc
                    else loop acc -- TODO validate
                c -> loop (c : acc)
        buffer <- T.pack . reverse <$> loop []
        pure (T.replace "\r" "\n" (T.replace "\r\n" "\n" buffer))

  pure
    RawReader
      { waitForInput = \singleEscAbort -> case pipeReader of
          Just _ -> select' singleEscAbort
          Nothing -> KeyPress <$> nextKey' singleEscAbort,
        nextKey = nextKey',
        nextChar = nextChar',
        readPastedText = readPasted,
        findBinding = \key -> Map.lookup key keyMap,
        unbuffer = do
          buf <- readIORef bufRef
          pure (if BS.null buf then Nothing else Just buf),
        pollInput = poll',
        releaseReader = mapM_ sigUninstall sig
      }
  where
    fkeys =
      [ ("11", KF 1), -- rxvt-unicode
        ("12", KF 2),
        ("13", KF 3),
        ("14", KF 4),
        ("15", KF 5), -- kf5
        ("17", KF 6),
        ("18", KF 7),
        ("19", KF 8),
        ("20", KF 9),
        ("21", KF 10),
        ("23", KF 11),
        ("24", KF 12)
      ]
    ctrlFkeys = filter ((`notElem` ["11", "12", "13", "14"]) . fst) fkeys

-- | Modifier digit of an xterm CSI sequence.
modifierFor :: Char -> Maybe Modifiers
modifierFor = \case
  '2' -> Just SHIFT
  '3' -> Just ALT
  '4' -> Just ALT_SHIFT
  '5' -> Just CTRL
  '6' -> Just CTRL_SHIFT
  '7' -> Just CTRL_ALT
  '8' -> Just CTRL_ALT_SHIFT
  _ -> Nothing

-- | \E[1;<mod><key>
csiModified :: Char -> Char -> KeyEvent
csiModified seq4 seq5 = fromMaybe (KeyEvent KUnknownEscSeq NONE) $ case seq4 of
  -- Meta + arrow on (some?) Macs when using iTerm defaults
  '9' -> (`KeyEvent` ALT) <$> arrow
  _ -> do
    m <- modifierFor seq4
    let digitKey = case seq5 of
          c | c >= 'p' && c <= 'y' && m /= SHIFT && m /= ALT && m /= ALT_SHIFT -> Just (KChar (chr (ord c - ord 'p' + ord '0')))
          _ -> Nothing
        ctrlF = case (m, seq5) of
          (CTRL, 'P') -> Just (KF 1)
          (CTRL, 'Q') -> Just (KF 2)
          (CTRL, 'S') -> Just (KF 4)
          _ -> Nothing
        homeEnd = case seq5 of
          'F' -> Just KEnd
          'H' -> Just KHome
          _ -> Nothing
    k <- case (arrow, homeEnd, ctrlF, digitKey) of
      (Just a, _, _, _) -> Just a
      (_, Just h, _, _) -> Just h
      (_, _, Just f, _) -> Just f
      (_, _, _, Just d) -> Just d
      _ -> Nothing
    pure (KeyEvent k m)
  where
    arrow = case seq5 of
      'A' -> Just KUp
      'B' -> Just KDown
      'C' -> Just KRight
      'D' -> Just KLeft
      _ -> Nothing

-- | \E[<key>;<mod>~
tildeModified :: Char -> Char -> KeyEvent
tildeModified seq2 seq4 = fromMaybe (KeyEvent KUnknownEscSeq NONE) $ do
  k <- case seq2 of
    '2' -> Just KInsert
    '3' -> Just KDelete
    '5' -> Just KPageUp
    '6' -> Just KPageDown
    _ -> Nothing
  m <- modifierFor seq4
  pure (KeyEvent k m)

readDigitsUntil :: RawReader -> Char -> IO (Maybe Int)
readDigitsUntil rdr sep = go 0
  where
    go :: Int -> IO (Maybe Int)
    go num =
      nextChar rdr >>= \case
        d | isDigit d -> go (min 4294967295 (num * 10 + (ord d - ord '0')))
        c | c == sep -> pure (Just num)
        _ -> pure Nothing

writeAllH :: Handle -> Text -> IO ()
writeAllH h t = BS.hPut h (TE.encodeUtf8 t) >> hFlush h

newPosixRenderer :: Handle -> Fd -> Unit -> Bool -> Bool -> GraphemeClusterMode -> BellStyle -> IO Renderer
newPosixRenderer out outFd tabStop' colors syncOutput gcm bell = do
  colsRef <- newIORef . fst =<< getWinSize outFd
  -- 0 when BSU is first used or after last ESU
  syncRef <- newIORef (0 :: Int)
  let write = writeAllH out

      clearOldRows layout =
        let currentRow = posRow (layoutCursor layout)
            oldRows = posRow (layoutEnd layout)
            -- old_rows < cursor_row if the prompt spans multiple lines and
            -- if this is the default State.
            cursorRowMovement = max 0 (oldRows - currentRow)
         in -- move the cursor down as required
            (if cursorRowMovement > 0 then "\x1b[" <> tshow cursorRowMovement <> "B" else "")
              -- clear old rows
              <> T.replicate oldRows "\r\x1b[K\x1b[A"
              -- clear the line
              <> "\r\x1b[K"

      beginSync = when syncOutput $ do
        n <- readIORef syncRef
        when (n == 0) $ write beginSynchronizedUpdateSeq
        writeIORef syncRef (n + 1)

      endSync = when syncOutput $ do
        n <- max 0 . subtract 1 <$> readIORef syncRef
        writeIORef syncRef n
        when (n == 0) $ write endSynchronizedUpdateSeq

      calcPos s orig = do
        cols <- readIORef colsRef
        let step (p, escSeq) c
              | c == "\n" = (Position 0 (posRow p + 1), escSeq)
              | otherwise =
                  let (cw, escSeq')
                        | c == "\t" = (tabStop' - (posCol p `mod` tabStop'), escSeq)
                        | otherwise = escWidth gcm c escSeq
                      col = posCol p + cw
                      p'
                        | col > cols = Position cw (posRow p + 1)
                        | otherwise = p {posCol = col}
                   in (p', escSeq')
            (end, _) = foldl step (orig, 0 :: Int) (graphemes s)
        pure $
          if posCol end == cols
            then Position 0 (posRow end + 1)
            else end

      moveCursor' old new =
        let rowPart
              | posRow new > posRow old = shiftSeq (posRow new - posRow old) 'B' -- move down
              | posRow new < posRow old = shiftSeq (posRow old - posRow new) 'A' -- move up
              | otherwise = ""
            colPart
              | posCol new > posCol old = shiftSeq (posCol new - posCol old) 'C' -- move right
              | posCol new < posCol old = shiftSeq (posCol old - posCol new) 'D' -- move left
              | otherwise = ""
            shiftSeq 1 c = "\x1b[" <> T.singleton c
            shiftSeq n c = "\x1b[" <> tshow n <> T.singleton c
         in write (rowPart <> colPart)

      refreshLine' p line hint oldLayout newLayout' hl = do
        beginSync
        let cursor = layoutCursor newLayout'
            endPos = layoutEnd newLayout'
            text = asStr line
        (promptText, lineText, hintText) <- case hl of
          Just h -> do
            pt <- highlightPrompt h (promptStyled p) (layoutDefaultPrompt newLayout')
            lt <- highlight h text (pos line)
            ht <- traverse (highlightHint h) hint
            pure (pt, lt, ht)
          Nothing -> pure (promptRaw p, text, hint)
        let endsWithNewline = maybe ("\n" `T.isSuffixOf` text) ("\n" `T.isSuffixOf`) hint
            rowMovement = posRow endPos - posRow cursor
            buffer =
              maybe "" clearOldRows oldLayout
                <> promptText
                <> lineText
                <> fromMaybe "" hintText
                -- we have to generate our own newline on line wrap
                <> (if posCol endPos == 0 && posRow endPos > 0 && not endsWithNewline then "\n" else "")
                -- move the cursor up as required
                <> (if rowMovement > 0 then "\x1b[" <> tshow rowMovement <> "A" else "")
                -- position the cursor within the line
                <> (if posCol cursor > 0 then "\r\x1b[" <> tshow (posCol cursor) <> "C" else "\r")
        write buffer
        endSync

      moveAtLeftmost rdr = do
        busy <- pollInput rdr 0
        unless busy $ do
          -- Report cursor location
          write "\x1b[6n"
          -- Read the response: ESC [ rows ; cols R
          ready <- pollInput rdr 100
          ok <-
            if not ready
              then pure False
              else do
                c1 <- nextChar rdr
                if c1 /= '\x1b'
                  then pure False
                  else do
                    c2 <- nextChar rdr
                    if c2 /= '['
                      then pure False
                      else isJust <$> readDigitsUntil rdr ';'
          when ok $ do
            col <- readDigitsUntil rdr 'R'
            when (col /= Just 1) $ write "\n"

  pure
    Renderer
      { moveCursor = moveCursor',
        refreshLine = refreshLine',
        calculatePosition = calcPos,
        writeAndFlush = write,
        beep = when (bell == Audible) (write "\x07"),
        clearScreen = write "\x1b[H\x1b[J",
        clearRows = write . clearOldRows,
        clearToEol = write "\x1b[K",
        updateSize = writeIORef colsRef . fst =<< getWinSize outFd,
        getColumns = readIORef colsRef,
        getRows = snd <$> getWinSize outFd,
        colorsEnabled = colors,
        rendererGcm = gcm,
        moveCursorAtLeftmost = moveAtLeftmost,
        beginSynchronizedUpdate = beginSync,
        endSynchronizedUpdate = endSync
      }

tshow :: Int -> Text
tshow = T.pack . show

enableRaw :: Fd -> Bool -> IO (TerminalAttributes, KeyMap)
enableRaw ttyIn enableSignals' = do
  original <- getTerminalAttributes ttyIn
  let raw0 =
        foldl
          withoutMode
          original
          -- disable BREAK interrupt, CR to NL conversion on input, input
          -- parity check, strip high bit (bit 8), output flow control
          [ InterruptOnBreak,
            MapCRtoLF,
            CheckParity,
            StripHighBit,
            StartStopOutput,
            -- disable echoing, canonical mode, extended input processing
            -- and signals
            EnableEcho,
            ProcessInput,
            ExtendedFunctions,
            KeyboardInterrupts
          ]
      -- character-size mark (8 bits)
      raw1 = withBits raw0 8
      raw2 = if enableSignals' then withMode raw1 KeyboardInterrupts else raw1
      -- one character-at-a-time input with blocking read
      raw = withTime (withMinInput raw2 1) 0
      keyMap =
        Map.fromList . catMaybes $
          [ (\c -> (newKey c NONE, cmd)) <$> controlChar raw cc
          | (cc, cmd) <-
              [ (PT.EndOfFile, EndOfFile),
                (PT.Interrupt, Interrupt),
                (PT.Quit, Interrupt),
                (PT.Suspend, Suspend)
              ]
          ]
  setTerminalAttributes ttyIn raw WhenDrained
  pure (original, keyMap)

-- | The POSIX terminal. With 'PreferTerm', @/dev/tty@ is used when it can
--   be opened.
newPosixTerminal :: Config -> IO Terminal
newPosixTerminal config = do
  devTty <-
    if behavior config == PreferTerm
      then either (const Nothing) Just <$> try @IOException (openFd "/dev/tty" ReadWrite defaultFileFlags)
      else pure Nothing
  (ttyIn, ttyOut, outHandle) <- case devTty of
    Just fd -> do
      h <- fdToHandle =<< dup fd
      pure (fd, fd, h)
    Nothing -> pure (Fd 0, Fd 1, stdout)
  isInTty <- queryTerminal ttyIn
  isOutTty <- queryTerminal ttyOut
  unsupported <- isUnsupportedTerm
  rawModeRef <- newIORef False
  pipesRef <- newIORef Nothing
  colorMode' <- effectiveColorMode config
  gcm <- resolveGraphemeClusterMode config
  let colors = case colorMode' of
        Enabled -> isOutTty
        Forced -> True
        Disabled -> False
      notTty = throwIO (ReadlineIo (errnoToIOError "rustyline" eNOTTY Nothing Nothing))
  pure
    Terminal
      { termIsUnsupported = unsupported,
        termIsInputTty = isInTty,
        termIsOutputTty = isOutTty,
        enableRawMode = \c -> do
          unless isInTty notTty
          (original, keyMap) <- enableRaw ttyIn (enableSignals c)
          writeIORef rawModeRef True
          -- enable bracketed paste
          out <-
            if not (enableBracketedPaste c)
              then pure False
              else either (const False) (const True) <$> try @IOException (writeAllH outHandle bracketedPasteOn)
          let disable = do
                setTerminalAttributes ttyIn original WhenDrained
                -- disable bracketed paste
                when out $ writeAllH outHandle bracketedPasteOff
                writeIORef rawModeRef False
          pure (disable, keyMap),
        createReader = \buffer c keyMap -> do
          sig <- if isOutTty then Just <$> installSigHandlers else pure Nothing
          pipes <- readIORef pipesRef
          newPosixReader ttyIn sig buffer c keyMap (fst <$> pipes),
        createWriter = \c ->
          newPosixRenderer
            outHandle
            ttyOut
            (fromIntegral (tabStop c))
            colors
            (enableSynchronizedOutput c)
            gcm
            (bellStyle c),
        writeln = writeAllH outHandle "\n",
        createExternalPrinter = do
          existing <- readIORef pipesRef
          PipeWriter w slot <- case existing of
            Just (_, writer) -> pure writer
            Nothing -> do
              when (unsupported || not isInTty || not isOutTty) notTty
              (r, w) <- createPipe
              slot <- newEmptyMVar
              let writer = PipeWriter w slot
              writeIORef pipesRef (Just (PipeReader r slot, writer))
              pure writer
          pure . ExternalPrinter $ \msg -> do
            -- write directly to the terminal while not in raw mode
            raw <- readIORef rawModeRef
            if not raw
              then writeAllH outHandle msg
              else do
                putMVar slot msg
                writeByte w (fromIntegral (ord 'm')),
        setCursorVisibility = \visible ->
          if not isOutTty
            then pure Nothing
            else do
              writeAllH outHandle (if visible then "\x1b[?25h" else "\x1b[?25l")
              pure (if visible then Nothing else Just (writeAllH outHandle "\x1b[?25h")),
        -- suspend the whole process group
        suspendProcess = signalProcess sigTSTP 0,
        termTestBindings = False
      }
