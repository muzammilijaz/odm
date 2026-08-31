import { useEffect, useRef } from "react";
import type { Task, TaskWithSpeed } from "./types";

interface Sample {
  bytes: number;
  at: number;
}

/** Derives an instantaneous download speed per task by diffing
 * downloaded_bytes between consecutive snapshots — the backend only
 * persists byte counts, not speed, so this stays purely client-side.
 *
 * The previous-sample map is only ever written inside a `useEffect`
 * (after render/commit), never during the computation itself. React's
 * `StrictMode` (enabled in dev, see main.tsx) intentionally double-invokes
 * render-phase code to catch exactly this class of bug: an earlier version
 * mutated the map directly while computing speeds, so on every update
 * StrictMode's second pass saw the first pass's just-written sample against
 * an unchanged `tasks` snapshot — a ~0ms elapsed time -- and overwrote the
 * correct speed with 0, permanently showing "—" for every active download
 * in dev. Keeping the read pure and only committing the new sample in an
 * effect means both passes read the same (stale, correct) previous sample
 * and compute the same real answer. */
export function useTaskSpeeds(tasks: Task[]): TaskWithSpeed[] {
  const lastSample = useRef<Map<number, Sample>>(new Map());
  const now = performance.now();

  const withSpeeds = tasks.map((task) => {
    const prev = lastSample.current.get(task.id);
    let bytesPerSec = 0;
    if (task.status === "Downloading" && prev) {
      const elapsedSec = (now - prev.at) / 1000;
      if (elapsedSec > 0) {
        bytesPerSec = Math.max(0, (task.downloaded_bytes - prev.bytes) / elapsedSec);
      }
    }
    return { ...task, bytesPerSec };
  });

  useEffect(() => {
    const next = new Map<number, Sample>();
    for (const task of tasks) {
      next.set(task.id, { bytes: task.downloaded_bytes, at: now });
    }
    lastSample.current = next;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tasks]);

  return withSpeeds;
}
