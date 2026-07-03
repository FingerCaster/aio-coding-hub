import { useCallback, useEffect, useMemo } from "react";

type UseCoalescedAsyncRefreshOptions<TSource, TResult> = {
  enabled: boolean;
  delayMs: number;
  task: (source: TSource) => Promise<TResult>;
  onError?: (error: unknown, source: TSource) => TResult | Promise<TResult>;
};

type CoalescedAsyncRefreshRuntime<TSource, TResult> = {
  initialized: boolean;
  timer: number | null;
  queued: boolean;
  queuedSource: TSource | undefined;
  queuedGeneration: number;
  inFlight: boolean;
  active: boolean;
  previousEnabled: boolean;
  enabledGeneration: number;
  task: (source: TSource) => Promise<TResult>;
  onError: ((error: unknown, source: TSource) => TResult | Promise<TResult>) | undefined;
  flush: ((source: TSource) => Promise<TResult | null> | null) | null;
};

export function useCoalescedAsyncRefresh<TSource, TResult = unknown>({
  enabled,
  delayMs,
  task,
  onError,
}: UseCoalescedAsyncRefreshOptions<TSource, TResult>) {
  const runtime = useMemo<CoalescedAsyncRefreshRuntime<TSource, TResult>>(
    () => ({
      initialized: false,
      timer: null,
      queued: false,
      queuedSource: undefined,
      queuedGeneration: 0,
      inFlight: false,
      active: false,
      previousEnabled: false,
      enabledGeneration: 0,
      task: async () => {
        throw new Error("Refresh task called before initialization.");
      },
      onError: undefined,
      flush: null,
    }),
    []
  );
  if (!runtime.initialized) {
    runtime.initialized = true;
    runtime.active = enabled;
    runtime.previousEnabled = enabled;
  }
  runtime.task = task;
  runtime.onError = onError;

  const clearQueuedForGeneration = useCallback(
    (generation: number) => {
      if (runtime.timer != null) {
        window.clearTimeout(runtime.timer);
        runtime.timer = null;
      }
      runtime.queued = false;
      runtime.queuedSource = undefined;
      runtime.queuedGeneration = generation;
    },
    [runtime]
  );
  const clearQueued = useCallback(() => {
    clearQueuedForGeneration(runtime.enabledGeneration);
  }, [clearQueuedForGeneration, runtime]);

  runtime.active = enabled;
  if (runtime.previousEnabled !== enabled) {
    runtime.previousEnabled = enabled;
    if (!enabled) {
      runtime.enabledGeneration += 1;
      clearQueued();
    }
  }

  const runTask = useCallback(
    async (source: TSource): Promise<TResult> => {
      try {
        return await runtime.task(source);
      } catch (error) {
        if (runtime.onError) {
          return await runtime.onError(error, source);
        }
        throw error;
      }
    },
    [runtime]
  );

  const flush = useCallback(
    (source: TSource): Promise<TResult | null> | null => {
      if (!runtime.active) {
        clearQueued();
        return null;
      }

      if (runtime.inFlight) {
        runtime.queued = true;
        runtime.queuedSource = source;
        runtime.queuedGeneration = runtime.enabledGeneration;
        return null;
      }

      runtime.queued = false;
      runtime.queuedSource = undefined;
      runtime.inFlight = true;

      return runTask(source).finally(() => {
        runtime.inFlight = false;
        if (
          !runtime.queued ||
          !runtime.active ||
          runtime.queuedGeneration !== runtime.enabledGeneration
        ) {
          runtime.queued = false;
          runtime.queuedSource = undefined;
          return;
        }

        const nextSource = runtime.queuedSource as TSource;
        runtime.queued = false;
        runtime.queuedSource = undefined;
        void runtime.flush?.(nextSource);
      });
    },
    [clearQueued, runTask, runtime]
  );

  runtime.flush = flush;

  const schedule = useCallback(
    (source: TSource) => {
      if (!runtime.active) {
        return;
      }

      if (runtime.timer != null) {
        runtime.queued = true;
        runtime.queuedSource = source;
        runtime.queuedGeneration = runtime.enabledGeneration;
        return;
      }

      const scheduledGeneration = runtime.enabledGeneration;
      const timerId = window.setTimeout(() => {
        if (runtime.timer === timerId) {
          runtime.timer = null;
        }
        if (scheduledGeneration !== runtime.enabledGeneration || !runtime.active) {
          return;
        }
        void flush(source);
      }, delayMs);
      runtime.timer = timerId;
    },
    [delayMs, flush, runtime]
  );

  useEffect(() => {
    return () => {
      runtime.active = false;
      runtime.inFlight = false;
      clearQueuedForGeneration(-1);
    };
  }, [clearQueuedForGeneration, runtime]);

  return {
    clearQueued,
    flush,
    schedule,
  };
}
