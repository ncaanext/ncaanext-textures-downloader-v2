import { useEffect, useRef, useState } from "react";

interface ProgressDisplayProps {
  messages: string[];
  percent: number | null;
  stage: string | null;
  isComplete?: boolean;
}

const STAGE_LABELS: Record<string, string> = {
  preparing: "Preparing installation...",
  cloning: "Initializing repository...",
  compressing: "Compressing for download...",
  downloading: "Downloading textures...",
  extracting: "Extracting textures...",
  moving: "Moving files to final location...",
  cleanup: "Cleaning up temporary files...",
  complete: "Installation complete!",
};

// Stages that show indeterminate progress (no percentage available)
const INDETERMINATE_STAGES = ["preparing", "moving", "cleanup"];

function ProgressDisplay({ messages, percent, stage, isComplete }: ProgressDisplayProps) {
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const [elapsedTime, setElapsedTime] = useState(0);
  const startTimeRef = useRef<number>(Date.now());

  // Auto-scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // Track elapsed time
  useEffect(() => {
    if (isComplete) return;

    const interval = setInterval(() => {
      setElapsedTime(Math.floor((Date.now() - startTimeRef.current) / 1000));
    }, 1000);

    return () => clearInterval(interval);
  }, [isComplete]);

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
  };

  // Determine if we should show indeterminate progress (pulsing animation)
  // Show indeterminate only for stages without progress AND when we don't have a percentage
  const isIndeterminate = stage !== null &&
    (INDETERMINATE_STAGES.includes(stage) || (percent === null || percent === 0));

  const stageLabel = stage ? STAGE_LABELS[stage] || stage : "Starting...";
  const displayPercent = percent ?? 0;

  return (
    <div className="mt-6 space-y-3">
      {/* Current stage indicator */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          {!isComplete && (
            <svg
              className="animate-spin h-4 w-4 text-blue-400"
              xmlns="http://www.w3.org/2000/svg"
              fill="none"
              viewBox="0 0 24 24"
            >
              <circle
                className="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                strokeWidth="4"
              />
              <path
                className="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
              />
            </svg>
          )}
          {isComplete && (
            <svg className="h-4 w-4 text-green-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
            </svg>
          )}
          <span className={`text-sm font-medium ${isComplete ? "text-green-400" : "text-zinc-200"}`}>
            {stageLabel}
          </span>
        </div>
        <span className="text-xs text-zinc-500">
          Elapsed: {formatTime(elapsedTime)}
        </span>
      </div>

      {/* Progress bar */}
      <div className="space-y-1">
        <div className="flex justify-between text-xs text-zinc-400">
          <span>Progress</span>
          <span>
            {isComplete
              ? "100%"
              : isIndeterminate && displayPercent === 0
                ? "Working..."
                : `${displayPercent}%`}
          </span>
        </div>
        <div className="w-full h-2 bg-zinc-700 rounded-full overflow-hidden">
          {isIndeterminate && !isComplete && displayPercent === 0 ? (
            // Animated indeterminate progress bar
            <div className="h-full w-full relative">
              <div
                className="absolute h-full bg-blue-500 animate-pulse"
                style={{ width: "100%", opacity: 0.5 }}
              />
              <div
                className="absolute h-full bg-blue-400 rounded-full animate-indeterminate"
                style={{ width: "30%" }}
              />
            </div>
          ) : (
            <div
              className={`h-full transition-all duration-300 ${
                isComplete ? "bg-green-500" : "bg-blue-500"
              }`}
              style={{ width: `${isComplete ? 100 : displayPercent}%` }}
            />
          )}
        </div>
      </div>

      {/* Terminal-style output */}
      <div className="bg-zinc-950 border border-zinc-700 rounded-lg p-3 max-h-48 overflow-y-auto font-mono text-xs">
        <div className="text-zinc-400 mb-2 pb-2 border-b border-zinc-800">
          Git Output:
        </div>
        {messages.length === 0 ? (
          <p className="text-zinc-500">Waiting for output...</p>
        ) : (
          <div className="space-y-0.5">
            {messages.map((msg, i) => (
              <p
                key={i}
                className={`${
                  msg.includes("complete") || msg.includes("Complete") || msg.includes("done")
                    ? "text-green-400"
                    : msg.includes("error") || msg.includes("Error")
                    ? "text-red-400"
                    : "text-zinc-300"
                }`}
              >
                {msg}
              </p>
            ))}
            <div ref={messagesEndRef} />
          </div>
        )}
      </div>

      {/* Completion message */}
      {isComplete && (
        <div className="bg-green-900/30 border border-green-700 rounded-lg p-3 text-green-300 text-sm">
          Installation completed successfully!
        </div>
      )}
    </div>
  );
}

export default ProgressDisplay;
