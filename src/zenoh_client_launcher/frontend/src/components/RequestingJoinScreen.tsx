import { invoke } from "@tauri-apps/api/core";
import type { JoinFlowDto } from "../types";

interface Props {
  joinFlow: JoinFlowDto | null;
  lastLog?: string;
}

/**
 * Visible while the requester's `JoinRequest` is sitting in the admin's
 * pending queue. Transitions:
 *  - on Allow → backend installs `GroupReceiver` and the snapshot flips
 *    to AppScreen::Main (handled by App.tsx).
 *  - on Deny  → user clicks "Back" to reach the Chooser (logout for now,
 *    until #110's "Back to chooser" flow is wired up).
 */
export default function RequestingJoinScreen({ joinFlow, lastLog }: Props) {
  const status = joinFlow?.status ?? "pending";
  const adminCn =
    joinFlow?.status === "pending" ? joinFlow.admin_cn : undefined;
  const denyReason =
    joinFlow?.status === "denied" ? joinFlow.reason : null;

  return (
    <div className="flex h-full items-center justify-center bg-gray-950">
      <div className="w-full max-w-md p-6 space-y-5 text-center">
        {status === "pending" && (
          <>
            <div className="inline-block w-10 h-10 border-2 border-zenoh-500 border-t-transparent rounded-full animate-spin" />
            <h1 className="text-xl font-semibold text-gray-100">
              Waiting for admission
            </h1>
            <p className="text-sm text-gray-400">
              Your request to join{" "}
              <span className="font-mono text-gray-200">{adminCn ?? "…"}</span>{" "}
              has been sent. The admin needs to allow you in.
            </p>
            {lastLog && (
              <div className="font-mono text-xs text-gray-600 truncate">
                {lastLog}
              </div>
            )}
          </>
        )}

        {status === "denied" && (
          <>
            <div className="text-5xl">⛔</div>
            <h1 className="text-xl font-semibold text-gray-100">
              Request denied
            </h1>
            {denyReason && (
              <p className="text-sm text-gray-400">{denyReason}</p>
            )}
            <button onClick={() => invoke("logout")} className="btn-secondary">
              Back to sign-in
            </button>
          </>
        )}

        {status === "timed_out" && (
          <>
            <div className="text-5xl">⏱️</div>
            <h1 className="text-xl font-semibold text-gray-100">
              Request timed out
            </h1>
            <p className="text-sm text-gray-400">
              The admin didn't respond. Try again later.
            </p>
            <button onClick={() => invoke("logout")} className="btn-secondary">
              Back to sign-in
            </button>
          </>
        )}
      </div>
    </div>
  );
}
