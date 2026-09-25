import { cn } from "@/lib/utils";

export interface ConfigDiffLine {
  kind: "context" | "removed" | "added";
  oldLineNumber: number | null;
  newLineNumber: number | null;
  text: string;
}

interface SplitRow {
  current?: ConfigDiffLine;
  proposed?: ConfigDiffLine;
}

function splitRows(lines: ConfigDiffLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let removed: ConfigDiffLine[] = [];
  let added: ConfigDiffLine[] = [];
  const flush = () => {
    for (
      let index = 0;
      index < Math.max(removed.length, added.length);
      index++
    ) {
      rows.push({ current: removed[index], proposed: added[index] });
    }
    removed = [];
    added = [];
  };
  for (const line of lines) {
    if (line.kind === "context") {
      flush();
      rows.push({ current: line, proposed: line });
    } else {
      (line.kind === "removed" ? removed : added).push(line);
    }
  }
  flush();
  return rows;
}

function lineStyle(line?: ConfigDiffLine) {
  return line?.kind === "removed"
    ? "bg-red-500/10 text-red-700 dark:text-red-300"
    : line?.kind === "added"
      ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
      : !line
        ? "bg-muted/40"
        : "";
}

function LineText({ line }: { line?: ConfigDiffLine }) {
  return (
    <div className="flex min-w-0">
      <span aria-hidden className="w-5 shrink-0 select-none text-center">
        {line?.kind === "removed" ? "−" : line?.kind === "added" ? "+" : " "}
      </span>
      <pre className="min-w-0 flex-1 whitespace-pre-wrap break-words pr-3 font-mono">
        {line && line.kind !== "context" && (
          <span className="sr-only">
            {line.kind === "removed" ? "Removed: " : "Added: "}
          </span>
        )}
        {line?.text.replace(/\n$/, "") || "\u00a0"}
        {line && !line.text.endsWith("\n") && (
          <span className="block select-none text-[10px] italic text-muted-foreground">
            \ No newline at end of file
          </span>
        )}
      </pre>
    </div>
  );
}

export function ConfigDiff({
  lines,
  layout,
}: {
  lines: ConfigDiffLine[];
  layout: "split" | "inline";
}) {
  const unchanged = lines.every((line) => line.kind === "context");
  const numberStyle =
    "select-none px-2 text-right align-top font-mono text-muted-foreground";
  return (
    <div
      role="region"
      aria-label="Configuration diff"
      className="overflow-auto"
    >
      {unchanged && (
        <p className="border-b p-4 text-sm text-muted-foreground">
          No changes needed in this file.
        </p>
      )}
      {lines.length > 0 && (
        <table className="w-full min-w-[640px] table-fixed border-collapse text-xs leading-6">
          <caption className="sr-only">
            Current TOML compared with proposed TOML
          </caption>
          {layout === "split" ? (
            <>
              <thead className="border-b bg-muted/40">
                <tr>
                  <th
                    scope="col"
                    className="border-r px-4 py-2 text-left font-medium"
                  >
                    Current TOML
                  </th>
                  <th scope="col" className="px-4 py-2 text-left font-medium">
                    Proposed TOML
                  </th>
                </tr>
              </thead>
              <tbody>
                {splitRows(lines).map((row, index) => (
                  <tr key={index}>
                    {(["current", "proposed"] as const).map((side) => {
                      const line = row[side];
                      return (
                        <td
                          key={side}
                          className={cn(
                            "p-0 align-top",
                            side === "current" && "border-r",
                            lineStyle(line),
                          )}
                        >
                          <div className="grid grid-cols-[3.5rem_minmax(0,1fr)]">
                            <span aria-hidden className={numberStyle}>
                              {side === "current"
                                ? line?.oldLineNumber
                                : line?.newLineNumber}
                            </span>
                            <LineText line={line} />
                          </div>
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </>
          ) : (
            <>
              <colgroup>
                <col className="w-14" />
                <col className="w-14" />
                <col />
              </colgroup>
              <thead className="border-b bg-muted/40">
                <tr>
                  <th
                    scope="col"
                    aria-label="Current line number"
                    className="px-2 py-2 text-right font-medium"
                  >
                    Old
                  </th>
                  <th
                    scope="col"
                    aria-label="Proposed line number"
                    className="px-2 py-2 text-right font-medium"
                  >
                    New
                  </th>
                  <th scope="col" className="px-4 py-2 text-left font-medium">
                    Current TOML → Proposed TOML
                  </th>
                </tr>
              </thead>
              <tbody>
                {lines.map((line, index) => (
                  <tr key={index} className={lineStyle(line)}>
                    <td className={numberStyle}>{line.oldLineNumber}</td>
                    <td className={numberStyle}>{line.newLineNumber}</td>
                    <td className="p-0 align-top">
                      <LineText line={line} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </>
          )}
        </table>
      )}
    </div>
  );
}
