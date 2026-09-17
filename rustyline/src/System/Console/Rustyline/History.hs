-- | History API (@history.rs@).
--
--   Rust's @&mut self@ histories become mutable handles: 'MemHistory' and
--   'FileHistory' wrap an 'IORef', and every 'History' method runs in 'IO'.
--   Errors are thrown as 'IOException's.
module System.Console.Rustyline.History
  ( SearchDirection (..),
    SearchResult (..),
    History (..),

    -- * In-memory history
    MemHistory,
    newMemHistory,
    memHistoryWithConfig,

    -- * File-backed history
    FileHistory,
    DefaultHistory,
    newFileHistory,
    fileHistoryWithConfig,
  )
where

import Control.Exception (bracket, try)
import Data.Bits ((.|.))
import Data.ByteString qualified as BS
import Data.ByteString.Builder qualified as B
import Data.Foldable (toList)
import Data.IORef
import Data.Sequence (Seq)
import Data.Sequence qualified as Seq
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Encoding qualified as TE
import Data.Time.Clock.POSIX (POSIXTime)
import GHC.IO.FD qualified as FD
import GHC.IO.Handle.FD (handleToFd)
import GHC.IO.Handle.Lock (LockMode (..), hLock)
import System.Console.Rustyline.Config (Config (..), HistoryDuplicates (..), defaultConfig)
import System.Console.Rustyline.Internal.Text (findSubB, lenB)
import System.Console.Rustyline.Internal.Unicode.CharProps (isWhitespace)
import System.Directory (doesPathExist)
import System.FilePath (normalise)
import System.IO
import System.Posix.Files (fileSize, getFdStatus, modificationTimeHiRes, ownerReadMode, ownerWriteMode, setFdMode)
import System.Posix.Files qualified as PF
import System.Posix.Types (Fd (..))

-- | Search direction.
data SearchDirection
  = -- | Search history forward
    SearchForward
  | -- | Search history backward
    SearchReverse
  deriving (Eq, Show)

-- | History search result.
data SearchResult = SearchResult
  { -- | history entry
    srEntry :: Text,
    -- | history index
    srIdx :: Int,
    -- | match position in 'srEntry' (byte offset)
    srPos :: Int
  }
  deriving (Eq, Show)

-- | Interface for navigating/loading/storing history.
class History h where
  -- | The entry at position @index@, starting from 0. 'SearchDirection' is
  --   useful only for implementations without direct indexing.
  historyGet :: h -> Int -> SearchDirection -> IO (Maybe SearchResult)

  -- | Add a new entry. False if the line has been ignored (blank line /
  --   duplicate / ...).
  historyAdd :: h -> Text -> IO Bool

  -- | Number of entries.
  historyLen :: h -> IO Int

  historyIsEmpty :: h -> IO Bool
  historyIsEmpty h = (== 0) <$> historyLen h

  -- | Set the maximum length, keeping just the latest entries.
  historySetMaxLen :: h -> Int -> IO ()

  -- | Ignore consecutive duplicates.
  historySetIgnoreDups :: h -> Bool -> IO ()

  -- | Ignore lines which begin with a space or not.
  historySetIgnoreSpace :: h -> Bool -> IO ()

  -- | Save the history in the specified file.
  historySave :: h -> FilePath -> IO ()

  -- | Append new entries in the specified file.
  historyAppend :: h -> FilePath -> IO ()

  -- | Load the history from the specified file. Throws if the path does not
  --   exist or could not be read.
  historyLoad :: h -> FilePath -> IO ()

  -- | Clear in-memory history.
  historyClear :: h -> IO ()

  -- | Search history (start position inclusive @[0, len-1]@). Returns the
  --   nearest entry containing @term@ between @[start, len-1]@ for forward
  --   search or @[0, start]@ for reverse search.
  historySearch :: h -> Text -> Int -> SearchDirection -> IO (Maybe SearchResult)

  -- | Anchored search.
  historyStartsWith :: h -> Text -> Int -> SearchDirection -> IO (Maybe SearchResult)

  -- | All entries, oldest first (Rust's @iter@).
  historyEntries :: h -> IO [Text]

-- ---------------------------------------------------------------------------
-- MemHistory

data MemState = MemState
  { msEntries :: Seq Text,
    msMaxLen :: Int,
    msIgnoreSpace :: Bool,
    msIgnoreDups :: Bool
  }

memStateWithConfig :: Config -> MemState
memStateWithConfig config =
  MemState
    { msEntries = Seq.empty,
      msMaxLen = maxHistorySize config,
      msIgnoreSpace = historyIgnoreSpace config,
      msIgnoreDups = historyDuplicates config == IgnoreConsecutive
    }

memGet :: MemState -> Int -> Maybe SearchResult
memGet ms index = (\entry -> SearchResult entry index 0) <$> Seq.lookup index (msEntries ms)

memLen :: MemState -> Int
memLen = Seq.length . msEntries

searchMatch :: MemState -> Text -> Int -> SearchDirection -> (Text -> Maybe Int) -> Maybe SearchResult
searchMatch ms term start dir test
  | T.null term || start >= memLen ms = Nothing
  | otherwise = firstJust (map check indices)
  where
    indices = case dir of
      SearchReverse -> [start, start - 1 .. 0]
      SearchForward -> [start .. memLen ms - 1]
    check idx =
      let entry = Seq.index (msEntries ms) idx
       in SearchResult entry idx <$> test entry
    firstJust = foldr (\x r -> maybe r Just x) Nothing

memIgnore :: MemState -> Text -> Bool
memIgnore ms line
  | msMaxLen ms == 0 = True
  | T.null line = True
  | msIgnoreSpace ms && maybe True (isWhitespace . fst) (T.uncons line) = True
  | msIgnoreDups ms, (_ Seq.:|> s) <- msEntries ms, s == line = True
  | otherwise = False

memInsert :: Text -> MemState -> MemState
memInsert line ms = ms {msEntries = entries Seq.|> line}
  where
    entries
      | memLen ms == msMaxLen ms = Seq.drop 1 (msEntries ms)
      | otherwise = msEntries ms

memAdd :: Text -> MemState -> (Bool, MemState)
memAdd line ms
  | memIgnore ms line = (False, ms)
  | otherwise = (True, memInsert line ms)

memSetMaxLen :: Int -> MemState -> MemState
memSetMaxLen n ms =
  ms
    { msMaxLen = n,
      msEntries = if memLen ms > n then Seq.drop (memLen ms - n) (msEntries ms) else msEntries ms
    }

memSearch :: MemState -> Text -> Int -> SearchDirection -> Maybe SearchResult
memSearch ms term start dir = searchMatch ms term start dir (findSubB term)

memStartsWith :: MemState -> Text -> Int -> SearchDirection -> Maybe SearchResult
memStartsWith ms term start dir = searchMatch ms term start dir test
  where
    test entry
      | term `T.isPrefixOf` entry = Just (lenB term)
      | otherwise = Nothing

-- | Transient in-memory history implementation.
newtype MemHistory = MemHistory (IORef MemState)

-- | A history configured with 'defaultConfig'.
newMemHistory :: IO MemHistory
newMemHistory = memHistoryWithConfig defaultConfig

-- | A history configured with 'maxHistorySize', 'historyIgnoreSpace' and
--   'historyDuplicates'.
memHistoryWithConfig :: Config -> IO MemHistory
memHistoryWithConfig config = MemHistory <$> newIORef (memStateWithConfig config)

unsupported :: String -> IO a
unsupported what = ioError (userError ("MemHistory: " <> what <> " is not supported"))

instance History MemHistory where
  historyGet (MemHistory ref) index _ = (`memGet` index) <$> readIORef ref
  historyAdd (MemHistory ref) line = atomicModifyIORef' ref (swap . memAdd line)
  historyLen (MemHistory ref) = memLen <$> readIORef ref
  historySetMaxLen (MemHistory ref) n = modifyIORef' ref (memSetMaxLen n)
  historySetIgnoreDups (MemHistory ref) yes = modifyIORef' ref (\ms -> ms {msIgnoreDups = yes})
  historySetIgnoreSpace (MemHistory ref) yes = modifyIORef' ref (\ms -> ms {msIgnoreSpace = yes})
  historySave _ _ = unsupported "save"
  historyAppend _ _ = unsupported "append"
  historyLoad _ _ = unsupported "load"
  historyClear (MemHistory ref) = modifyIORef' ref (\ms -> ms {msEntries = Seq.empty})
  historySearch (MemHistory ref) term start dir = (\ms -> memSearch ms term start dir) <$> readIORef ref
  historyStartsWith (MemHistory ref) term start dir = (\ms -> memStartsWith ms term start dir) <$> readIORef ref
  historyEntries (MemHistory ref) = toList . msEntries <$> readIORef ref

swap :: (a, b) -> (b, a)
swap (a, b) = (b, a)

-- ---------------------------------------------------------------------------
-- FileHistory

-- | Last history path, modified timestamp and size.
data PathInfo = PathInfo FilePath POSIXTime Int

data FileState = FileState
  { fsMem :: MemState,
    -- | Number of entries inputted by user and not saved yet
    fsNewEntries :: Int,
    -- | last path used by either load or save
    fsPathInfo :: Maybe PathInfo
  }

-- | History stored in a file.
--
--   Multiline-aware history files start with @#V2@ and have newlines and
--   backslashes escaped in them.
newtype FileHistory = FileHistory (IORef FileState)

-- | Default file-based history implementation.
type DefaultHistory = FileHistory

fileVersionV2 :: Text
fileVersionV2 = "#V2"

newFileHistory :: IO FileHistory
newFileHistory = fileHistoryWithConfig defaultConfig

fileHistoryWithConfig :: Config -> IO FileHistory
fileHistoryWithConfig config =
  FileHistory <$> newIORef (FileState (memStateWithConfig config) 0 Nothing)

handleFd :: Handle -> IO Fd
handleFd h = Fd . FD.fdFD <$> handleToFd h

fixPerm :: Handle -> IO ()
fixPerm h = do
  _ <- try @IOError (handleFd h >>= \fd -> setFdMode fd (ownerReadMode .|. ownerWriteMode))
  pure ()

-- | Write the entries (all of them, or only the new ones when appending).
saveTo :: FileState -> Handle -> Bool -> IO ()
saveTo st h append = do
  fixPerm h
  let mem = fsMem st
      (header, firstNewEntry)
        | append = (mempty, max 0 (memLen mem - fsNewEntries st))
        | otherwise = (B.byteString (TE.encodeUtf8 fileVersionV2) <> B.char7 '\n', 0)
      body = foldMap (\e -> escape e <> B.char7 '\n') (Seq.drop firstNewEntry (msEntries mem))
  B.hPutBuilder h (header <> body)
  hFlush h
  where
    escape = T.foldr (\c r -> esc c <> r) mempty
    esc '\n' = B.string7 "\\n" -- escaped line feed
    esc '\\' = B.string7 "\\\\" -- escaped backslash
    esc c = B.charUtf8 c

readRest :: Handle -> IO BS.ByteString
readRest h = go []
  where
    go acc = do
      chunk <- BS.hGetSome h 65536
      if BS.null chunk then pure (BS.concat (reverse acc)) else go (chunk : acc)

-- | Rust's @BufRead::lines@: split on @\\n@, drop one trailing @\\r@, no
--   final empty line; invalid UTF-8 is an error.
fileLines :: BS.ByteString -> IO [Text]
fileLines bytes = case TE.decodeUtf8' bytes of
  Left _ -> ioError (userError "stream did not contain valid UTF-8")
  Right t ->
    let ls = T.splitOn "\n" t
        ls' = if not (null ls) && T.null (last ls) then init ls else ls
     in pure (map stripCR ls')
  where
    stripCR l = maybe l id (T.stripSuffix "\r" l)

-- | Undo 'saveTo' escaping. A malformed line is kept verbatim.
unescapeLine :: Text -> Text
unescapeLine line
  | not (T.any (== '\\') line) = line
  | otherwise = maybe line T.concat (go line)
  where
    go s = case T.breakOn "\\" s of
      (before, rest)
        | T.null rest -> Just [before]
        | otherwise -> case T.uncons (T.drop 1 rest) of
            Just ('n', more) -> (before :) . ("\n" :) <$> go more
            Just ('\\', more) -> (before :) . ("\\" :) <$> go more
            -- only line feed and back slash should have been escaped
            _ -> Nothing

-- | Load entries, returning whether the file can later just be appended to.
loadFrom :: Handle -> FileState -> IO (Bool, FileState)
loadFrom h st0 = do
  ls <- fileLines =<< readRest h
  let add line st = let (ok, m) = memAdd line (fsMem st) in (ok, bumpNew ok st {fsMem = m})
      (v2, rest, st1) = case ls of
        [] -> (False, [], st0)
        (first : more)
          | first == fileVersionV2 -> (True, more, st0)
          | otherwise -> (False, more, snd (add first st0))
      step (appendable, st) line
        | T.null line = (appendable, st)
        | otherwise =
            let (ok, st') = add (if v2 then unescapeLine line else line) st
             in (appendable && ok, st')
      (appendable', st2) = foldl step (v2, st1) rest
  pure (appendable', st2 {fsNewEntries = 0})

bumpNew :: Bool -> FileState -> FileState
bumpNew False st = st
bumpNew True st = st {fsNewEntries = min (fsNewEntries st + 1) (memLen (fsMem st))}

modifiedTime :: Handle -> IO (POSIXTime, Integer)
modifiedTime h = do
  status <- getFdStatus =<< handleFd h
  pure (modificationTimeHiRes status, fromIntegral (fileSize status))

updatePath :: FilePath -> Handle -> Int -> FileState -> IO FileState
updatePath path h size st = do
  (modified, _) <- modifiedTime h
  pure st {fsPathInfo = Just (PathInfo path modified size)}

canJustAppend :: FilePath -> Handle -> FileState -> IO Bool
canJustAppend path h st = case fsPathInfo st of
  Nothing -> pure False
  Just (PathInfo previousPath previousModified previousSize)
    | normalise previousPath /= normalise path -> pure False
    | otherwise -> do
        (modified, _) <- modifiedTime h
        let maxLen = msMaxLen (fsMem st)
        pure $
          not
            ( previousModified /= modified
                || maxLen <= previousSize
                || maxLen < previousSize + fsNewEntries st
            )

-- | Create (or truncate) a file with permissions restricted to the owner.
createPrivate :: FilePath -> IO Handle
createPrivate path = do
  let mask = PF.ownerExecuteMode .|. PF.groupModes .|. PF.otherModes
  old <- PF.setFileCreationMask mask
  r <- try @IOError (openBinaryFile path WriteMode)
  _ <- PF.setFileCreationMask old
  either ioError pure r

fileSave :: FilePath -> FileState -> IO FileState
fileSave path st
  | memLen (fsMem st) == 0 || fsNewEntries st == 0 = pure st
  | otherwise = bracket (createPrivate path) hClose $ \h -> do
      hLock h ExclusiveLock
      saveTo st h False
      updatePath path h (memLen (fsMem st)) st {fsNewEntries = 0}

fileAppend :: FilePath -> FileState -> IO FileState
fileAppend path st
  | memLen (fsMem st) == 0 || fsNewEntries st == 0 = pure st
  | otherwise = do
      exists <- doesPathExist path
      if not exists || fsNewEntries st == msMaxLen (fsMem st)
        then fileSave path st
        else withBinaryFile path ReadWriteMode $ \h -> do
          hLock h ExclusiveLock
          justAppend <- canJustAppend path h st
          if justAppend
            then do
              hSeek h SeekFromEnd 0
              saveTo st h True
              let size = case fsPathInfo st of
                    Just (PathInfo _ _ s) -> s + fsNewEntries st
                    Nothing -> fsNewEntries st
              updatePath path h size st {fsNewEntries = 0}
            else do
              -- we may need to truncate file before appending new entries
              let mem = fsMem st
                  other0 = FileState mem {msEntries = Seq.empty} 0 Nothing
              (_, other1) <- loadFrom h other0
              let firstNewEntry = max 0 (memLen mem - fsNewEntries st)
                  other =
                    foldl
                      (\o e -> let (ok, m) = memAdd e (fsMem o) in bumpNew ok o {fsMem = m})
                      other1
                      (Seq.drop firstNewEntry (msEntries mem))
              hSeek h AbsoluteSeek 0
              hSetFileSize h 0 -- if new size < old size
              saveTo other h False
              st' <- updatePath path h (memLen (fsMem other)) st
              pure st' {fsNewEntries = 0}

fileLoad :: FilePath -> FileState -> IO FileState
fileLoad path st = withBinaryFile path ReadMode $ \h -> do
  hLock h SharedLock
  let before = memLen (fsMem st)
  (appendable, st') <- loadFrom h st
  if appendable
    then updatePath path h (memLen (fsMem st') - before) st'
    else -- discard old version on next save
      pure st' {fsPathInfo = Nothing}

withState :: IORef FileState -> (FileState -> IO FileState) -> IO ()
withState ref f = readIORef ref >>= f >>= writeIORef ref

instance History FileHistory where
  historyGet (FileHistory ref) index _ = (\st -> memGet (fsMem st) index) <$> readIORef ref
  historyAdd (FileHistory ref) line = atomicModifyIORef' ref $ \st ->
    let (ok, m) = memAdd line (fsMem st) in (bumpNew ok st {fsMem = m}, ok)
  historyLen (FileHistory ref) = memLen . fsMem <$> readIORef ref
  historySetMaxLen (FileHistory ref) n = modifyIORef' ref $ \st ->
    st {fsMem = memSetMaxLen n (fsMem st), fsNewEntries = min (fsNewEntries st) n}
  historySetIgnoreDups (FileHistory ref) yes =
    modifyIORef' ref (\st -> st {fsMem = (fsMem st) {msIgnoreDups = yes}})
  historySetIgnoreSpace (FileHistory ref) yes =
    modifyIORef' ref (\st -> st {fsMem = (fsMem st) {msIgnoreSpace = yes}})
  historySave (FileHistory ref) path = withState ref (fileSave path)
  historyAppend (FileHistory ref) path = withState ref (fileAppend path)
  historyLoad (FileHistory ref) path = withState ref (fileLoad path)
  historyClear (FileHistory ref) =
    modifyIORef' ref (\st -> st {fsMem = (fsMem st) {msEntries = Seq.empty}, fsNewEntries = 0})
  historySearch (FileHistory ref) term start dir =
    (\st -> memSearch (fsMem st) term start dir) <$> readIORef ref
  historyStartsWith (FileHistory ref) term start dir =
    (\st -> memStartsWith (fsMem st) term start dir) <$> readIORef ref
  historyEntries (FileHistory ref) = toList . msEntries . fsMem <$> readIORef ref

