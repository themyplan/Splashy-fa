interface ToggleProps {
  enabled: boolean;
  onChange: (enabled: boolean) => void;
}

export default function Toggle({ enabled, onChange }: ToggleProps) {
  return (
    <button
      onClick={() => onChange(!enabled)}
      style={{ width: "2.5rem", height: "1.375rem" }}
      className={`rounded-full relative transition-colors duration-200 ease-in-out cursor-pointer shrink-0 ${
        enabled ? "bg-white" : "bg-white/20"
      }`}
    >
      <span
        className={`absolute top-0.5 w-4 h-4 rounded-full shadow transition-[inset-inline-start,background-color] duration-200 ease-in-out ${
          enabled ? "start-[calc(100%-1.125rem)] bg-slate-900" : "start-0.5 bg-white"
        }`}
      />
    </button>
  );
}
