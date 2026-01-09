{-# LANGUAGE ApplicativeDo #-}
{-# LANGUAGE GeneralizedNewtypeDeriving #-}

module Idyllic.Build.Driver (runDefaultDriver, runDriver) where

import Control.Monad.Reader (MonadIO (liftIO), MonadReader, ReaderT (runReaderT), asks)
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Debug.Trace (trace)
import Idyllic.Build.Pipeline (runPipelineIO)
import Idyllic.Build.Pipeline.Effect
import Options.Applicative
import System.Console.Haskeline

newtype Driver a = Driver {unDriver :: ReaderT DriverEnv IO a}
  deriving (Functor, Applicative, Monad, MonadIO, MonadReader DriverEnv)

data DriverEnv = DriverEnv
  { driverArgs :: CliOptions,
    driverPaths :: SearchPaths,
    driverMode :: DriverMode
  }

data CliOptions = CliOptions
  { optMode :: DriverMode,
    optDebug :: Bool
  }
  deriving (Show, Eq)

data SearchPaths = SearchPaths
  { spSourcePaths :: [FilePath],
    spLibraryPaths :: [FilePath]
  }

data DriverMode
  = DriverModeFile FilePath
  | DriverModeInteractive
  deriving (Show, Eq)

-- | Parser for Mode: either a FILE argument, or no argument = REPL.
modeParser :: Parser DriverMode
modeParser =
  fmap toMode . optional $
    strArgument
      ( metavar "FILE"
          <> help "Source file to compile/run (omit to start REPL)"
      )
  where
    toMode :: Maybe FilePath -> DriverMode
    toMode (Just fp) = DriverModeFile fp
    toMode Nothing = DriverModeInteractive

-- | Parser for the whole Options record.
optionsParser :: Parser CliOptions
optionsParser = do
  optMode <- modeParser
  optDebug <-
    switch
      ( long "debug"
          <> short 'd'
          <> help "Enable debug logging"
      )
  pure CliOptions {..}

optionsInfo :: ParserInfo CliOptions
optionsInfo =
  info
    (optionsParser <**> helper)
    ( fullDesc
        <> progDesc "Welcome to the Idyll REPL!"
        <> header "Idyll"
    )

parseOptions :: IO CliOptions
parseOptions = execParser optionsInfo

runDefaultDriver :: IO ()
runDefaultDriver = do
  opts <- parseOptions
  let env =
        DriverEnv
          { driverArgs = opts,
            driverPaths = SearchPaths [] [],
            driverMode = optMode opts
          }
  runDriver env $ case driverMode env of
    DriverModeFile fp -> do
      src <- liftIO $ B.readFile fp
      debug <- asks (optDebug . driverArgs)
      liftIO $ runPipelineIO debug (InputModeFile $ T.pack fp) src
    DriverModeInteractive -> do
      liftIO $ putStrLn "Welcome to the Idyll REPL!"
      env' <- liftIO $ newPipelineEnv True InputModeInteractive B.empty
      liftIO $ runInputT settings (repl env')

runDriver :: DriverEnv -> Driver a -> IO a
runDriver env (Driver m) = runReaderT m env

runOneInput :: PipelineEnv -> String -> IO ()
runOneInput env inputStr = do
  let txt = T.pack inputStr
      bs = TE.encodeUtf8 txt
      debug = pipelineDebug env
  runPipelineIO debug InputModeInteractive bs

repl :: PipelineEnv -> InputT IO ()
repl env = loop
  where
    loop :: InputT IO ()
    loop = do
      minput <- getMultilineInput ""
      case minput of
        -- EOF (Ctrl+D)
        Nothing -> do
          outputStrLn "Exiting..."
          pure ()
        Just input
          | input == ":quit" || input == ":q" -> do
              outputStrLn "Exiting..."
              pure ()
          | all (`elem` (" \t\n\r" :: String)) input -> do
              -- empty / whitespace-only: just prompt again
              loop
          | otherwise -> do
              -- Run this input through the pipeline
              liftIO $ runOneInput env input
              loop

settings :: Settings IO
settings = defaultSettings {historyFile = Just ".repl_history"}

getMultilineInput :: String -> InputT IO (Maybe String)
getMultilineInput acc = do
  firstLine <- getInputLine "> "
  case firstLine of
    Nothing -> pure Nothing
    Just fl -> collectLines (acc ++ fl ++ "\n")

collectLines :: String -> InputT IO (Maybe String)
collectLines acc = do
  minput <- getInputLine ""
  case minput of
    Nothing -> pure Nothing
    Just "" -> pure $ Just (init acc)
    Just input -> collectLines (acc ++ input ++ "\n")

-- let (out, _) = runState (runPipeline (InputModeFile "main") (pack src)) defaultPipelineEnv
-- case out of
--   Left e ->
--     let diag = errorDiagnosticFromBundle Nothing ("Parse error on input" :: Text) Nothing e
--         diag' = addFile diag "main" src
--      in printDiagnostic stderr True True 2 defaultStyle diag'
--   Right prog -> putStrLn $ unpack . toStrict $ pShow prog