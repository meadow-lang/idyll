-- | The layout (offside) pass: turn significant indentation into explicit
--   block structure, so the parser can ignore whitespace entirely.
--
--   It runs in two stages.  'mark' annotates the token stream with the two
--   facts layout cares about — where a block /opens/ ('OpenBlock', carrying
--   the column its items will line up at) and where a line /begins/ ('Indent',
--   carrying that line's column).  'layout' then walks those markers with a
--   stack of open blocks, emitting the virtual 'TokenVLBrace' \/
--   'TokenVRBrace' \/ 'TokenVSemi' the parser consumes.
--
--   __How this differs from Haskell's rule.__  Haskell closes a layout block
--   purely by dedentation, plus the celebrated parse-error side condition.
--   Here, @let@ and @do@ instead record the keyword that closes them — @in@
--   and @where@ respectively — and that keyword closes the block wherever it
--   turns up (see 'closes').  It is a much simpler rule to implement, but it
--   is a genuine restriction: because @in@ closes the /innermost/ block that
--   is waiting for one, a @let@ cannot appear inside a @case@ alternative
--   without explicit braces.  The @in@ would be matched against the wrong
--   block.
module Idyllic.Syntax.Layout
  ( -- * Errors
    LayoutError (..),
    layoutErrorReport,

    -- * Marking
    Marked (..),
    mark,

    -- * Layout
    layout,
  )
where

import Control.Monad.State.Strict
import Data.Maybe (listToMaybe)
import Data.Text (Text)
import Data.Text qualified as T
import Error.Diagnose (Marker (..), Position (..), Report, err)
import Idyllic.Syntax.Token
import Idyllic.Utils

-- ---------------------------------------------------------------------------
-- Errors
-- ---------------------------------------------------------------------------

data LayoutError
  = -- | A block opened at a column that is not further right than the block
    --   containing it: expected greater than, got.
    NestedLayoutContextError Int Int Span
  | UnexpectedClosingBraceError Span
  | NotInLayoutContextAtEndOfInputError Span
  deriving (Show, Eq, Ord)

-- | Render a layout error the same way every other pass renders its errors.
layoutErrorReport :: FilePath -> LineIndex -> LayoutError -> Report Text
layoutErrorReport file li e = case e of
  NestedLayoutContextError outer inner sp ->
    mk sp $
      "nested layout context: expected indentation greater than "
        <> tshow outer
        <> ", but got "
        <> tshow inner
  UnexpectedClosingBraceError sp -> mk sp "unexpected closing brace"
  NotInLayoutContextAtEndOfInputError sp ->
    mk sp "not inside a layout context at end of input"
  where
    tshow :: (Show a) => a -> Text
    tshow = T.pack . show
    mk (Span s eoff) msg =
      let (l1, c1) = offsetToLineCol li s
          (l2, c2) = offsetToLineCol li eoff
       in err Nothing msg [(Position (l1, c1) (l2, c2) file, This msg)] []

-- ---------------------------------------------------------------------------
-- Heralds
-- ---------------------------------------------------------------------------

-- | What sort of block a keyword opens.
data Herald
  = -- | @let@ and @do@: closed by dedenting /or/ by an explicit keyword.
    ClosedBy Token
  | -- | @where@ and @of@: closed only by dedenting.
    OpenEnded

-- | The keywords that open a layout block.
heraldOf :: Token -> Maybe Herald
heraldOf = \case
  TokenLet -> Just (ClosedBy TokenLet)
  TokenDo -> Just (ClosedBy TokenDo)
  TokenWhere -> Just OpenEnded
  TokenOf -> Just OpenEnded
  _ -> Nothing

heraldCloser :: Herald -> Maybe Token
heraldCloser (ClosedBy t) = Just t
heraldCloser OpenEnded = Nothing

-- | @in@ closes a @let@; @where@ closes a @do@.  This is the token-driven
--   half of the algorithm described at the top of the module.
closes :: Token -> Token -> Bool
closes TokenIn TokenLet = True
closes TokenWhere TokenDo = True
closes _ _ = False

-- ---------------------------------------------------------------------------
-- Marking
-- ---------------------------------------------------------------------------

data Marked
  = Token LToken
  | OpenBlock Int (Maybe Token) Span
  | Indent Int Span
  deriving (Show, Eq, Ord)

mark :: LineIndex -> [LToken] -> [Marked]
mark li = markIndents li . markBlocks li

columnOf :: LineIndex -> LToken -> Int
columnOf li t = snd (offsetToLineCol li (spanStart (locSpan t)))

isNewline :: LToken -> Bool
isNewline t = unLoc t == TokenNewline

-- | Drop a leading run of newlines, so a block's column is taken from the
--   first token that actually carries content.
skipNewlines :: [LToken] -> [LToken]
skipNewlines (t : ts) | isNewline t = dropWhile isNewline ts
skipNewlines ts = ts

-- | Insert an 'OpenBlock' after every layout herald, recording the column of
--   the first token of the block.
markBlocks :: LineIndex -> [LToken] -> [Marked]
markBlocks li = go
  where
    go [] = []
    go (t : rest) = case heraldOf (unLoc t) of
      Just herald | opensBlock herald rest -> Token t : openBlock t herald rest
      _ -> Token t : go rest

    -- An explicit '{' means the block is written out by hand, so layout keeps
    -- out of it.  At end of input only 'let'/'do' open a (necessarily empty)
    -- block; a trailing 'where'/'of' opens nothing.
    opensBlock herald = \case
      [] -> case herald of ClosedBy _ -> True; OpenEnded -> False
      (n : _) -> unLoc n /= TokenLBrace

    openBlock t herald rest = case skipNewlines rest of
      [] -> [OpenBlock 0 closer (locSpan t)]
      (first : more) -> OpenBlock (columnOf li first) closer (locSpan t) : resume first more
      where
        closer = heraldCloser herald
        -- 'let'/'do' re-examine the block's first token, which may itself be a
        -- herald ("let x = do .."); 'where'/'of' take it as read.
        resume first more = case herald of
          ClosedBy _ -> go (first : more)
          OpenEnded -> Token first : go more

-- | Replace each newline with an 'Indent' carrying the column of the token
--   that follows it.
markIndents :: LineIndex -> [Marked] -> [Marked]
markIndents li = go
  where
    go [] = []
    -- an OpenBlock already knows its column; nothing to do
    go (m@OpenBlock {} : ms) = m : go ms
    go (m@(Token n) : ms)
      | isNewline n = case ms of
          -- a trailing newline indents nothing
          [] -> []
          (next@(Token t) : ms') -> Indent (columnOf li t) (locSpan n) : next : go ms'
          _ -> m : go ms
    go (m : ms) = m : go ms

-- ---------------------------------------------------------------------------
-- Layout
-- ---------------------------------------------------------------------------

-- | An open block.  'ctxColumn' is the column its items line up at, or 0 for
--   a block written with explicit braces — those are closed by @}@ rather
--   than by dedenting, which 'isExplicit' is asking about.
data LayoutCtx = LayoutCtx
  { ctxColumn :: Int,
    ctxCloser :: Maybe Token
  }

isExplicit :: LayoutCtx -> Bool
isExplicit ctx = ctxColumn ctx == 0

data LayoutState = LayoutState
  { -- | open blocks, innermost first
    lsStack :: [LayoutCtx],
    -- | emitted tokens, reversed
    lsOut :: [LToken],
    -- | errors, reversed
    lsErrors :: [LayoutError]
  }

type LayoutM = State LayoutState

emit :: LToken -> LayoutM ()
emit t = modify' $ \st -> st {lsOut = t : lsOut st}

report :: LayoutError -> LayoutM ()
report e = modify' $ \st -> st {lsErrors = e : lsErrors st}

push :: LayoutCtx -> LayoutM ()
push c = modify' $ \st -> st {lsStack = c : lsStack st}

pop :: LayoutM ()
pop = modify' $ \st -> st {lsStack = drop 1 (lsStack st)}

-- | The innermost open block, if any.
peek :: LayoutM (Maybe LayoutCtx)
peek = gets (listToMaybe . lsStack)

layout :: LineIndex -> [LToken] -> ([LToken], [LayoutError])
layout li input =
  ( Located TokenVLBrace (Span 0 0) : reverse (lsOut final),
    reverse (lsErrors final)
  )
  where
    final = execState (run (mark li input)) start
    start = LayoutState [LayoutCtx 1 Nothing] [] []

    -- Everything still open at end of input is closed at the last token.
    endSpan = lastSpan input
      where
        lastSpan [] = Span 0 0
        lastSpan [t] = locSpan t
        lastSpan (_ : ts) = lastSpan ts

    run :: [Marked] -> LayoutM ()
    run [] = closeAll
    run (m : ms) = case m of
      Indent col sp -> indent col sp ms
      OpenBlock col closer sp -> open col closer sp ms
      Token t -> token t ms

    -- An 'Indent' is where a line starts.  Against the innermost block's
    -- column it means: equal, a new item; further right, a continuation line;
    -- further left, the block has ended.
    indent :: Int -> Span -> [Marked] -> LayoutM ()
    indent col sp rest =
      peek >>= \case
        Nothing -> run rest
        Just ctx
          | col == ctxColumn ctx -> emit (Located TokenVSemi sp) >> run rest
          | col > ctxColumn ctx -> run rest
          | otherwise -> dedent ctx col sp rest

    -- Dedenting closes the innermost block.  If the token right after it is
    -- the keyword that also closes that block, it belongs just after the
    -- virtual '}' and is consumed here.
    dedent :: LayoutCtx -> Int -> Span -> [Marked] -> LayoutM ()
    dedent ctx col sp rest = do
      emit (Located TokenVRBrace sp)
      pop
      case (ctxCloser ctx, rest) of
        (Just closer, Token t : rest')
          | closes (unLoc t) closer -> emit t >> run rest'
        -- one dedent can close several blocks, so re-run this same Indent
        -- against whatever is now innermost
        _ -> indent col sp rest

    -- A block may only open further right than the block containing it.
    open :: Int -> Maybe Token -> Span -> [Marked] -> LayoutM ()
    open col closer sp rest =
      peek >>= \case
        Just ctx
          | col > ctxColumn ctx -> enter
          | otherwise -> do
              report (NestedLayoutContextError (ctxColumn ctx) col sp)
              pop
              run rest
        Nothing
          | col > 0 -> enter
          -- a herald with nothing after it: an empty block, '{}' at once
          | otherwise -> do
              emit (Located TokenVLBrace sp)
              emit (Located TokenVRBrace sp)
              indent col sp rest
      where
        enter = do
          emit (Located TokenVLBrace sp)
          push (LayoutCtx col closer)
          run rest

    token :: LToken -> [Marked] -> LayoutM ()
    token t rest
      | unLoc t == TokenRBrace = closeExplicit t rest
      -- 'let {' / 'do {': the user opens the block, but we still record which
      -- keyword closes it so a later 'in' can be matched up
      | Just closer <- heraldCloser =<< heraldOf (unLoc t),
        (lb : rest') <- rest,
        Token lbTok <- lb,
        unLoc lbTok == TokenLBrace = do
          emit t
          emit lbTok
          push (LayoutCtx 0 (Just closer))
          run rest'
      | unLoc t == TokenLBrace = do
          emit t
          push (LayoutCtx 0 Nothing)
          run rest
      | otherwise = closeByKeyword t rest

    -- '}' closes an explicit block; if that block was opened by 'let {' its
    -- closing keyword may follow immediately.
    closeExplicit :: LToken -> [Marked] -> LayoutM ()
    closeExplicit t rest =
      peek >>= \case
        Just ctx
          | isExplicit ctx -> case (ctxCloser ctx, rest) of
              (Nothing, _) -> emit t >> pop >> run rest
              (Just closer, Token kw : rest')
                | closes (unLoc kw) closer -> emit t >> emit kw >> pop >> run rest'
              _ -> unexpected
        _ -> unexpected
      where
        unexpected = do
          report (UnexpectedClosingBraceError (locSpan t))
          run rest

    -- 'in' / 'where' closing the block that is waiting for it.
    closeByKeyword :: LToken -> [Marked] -> LayoutM ()
    closeByKeyword t rest =
      peek >>= \case
        Just ctx
          | not (isExplicit ctx),
            Just closer <- ctxCloser ctx,
            closes (unLoc t) closer -> do
              emit (Located TokenVRBrace (locSpan t))
              emit t
              pop
              run rest
        _ -> emit t >> run rest

    -- At end of input every layout block is closed; an explicit block that
    -- never saw its '}' is an error, and stops the unwinding.
    closeAll :: LayoutM ()
    closeAll =
      peek >>= \case
        Nothing -> pure ()
        Just ctx
          | isExplicit ctx -> report (NotInLayoutContextAtEndOfInputError endSpan)
          | otherwise -> do
              emit (Located TokenVRBrace endSpan)
              pop
              closeAll
