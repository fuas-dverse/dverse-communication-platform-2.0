interface Props {
  lastLog?: string;
}

export default function LoadingScreen({ lastLog }: Props) {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-4">
      <h1 className="text-xl font-semibold text-gray-100">Connecting…</h1>
      {lastLog && (
        <p className="text-sm text-gray-500 max-w-xs text-center">{lastLog}</p>
      )}
      <div className="flex gap-1.5 mt-2">
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className="w-2 h-2 rounded-full bg-zenoh-500 animate-pulse"
            style={{ animationDelay: `${i * 0.2}s` }}
          />
        ))}
      </div>
    </div>
  );
}
