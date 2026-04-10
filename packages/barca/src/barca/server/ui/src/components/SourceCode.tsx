import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";

interface SourceCodeProps {
  code: string;
  title?: string;
}

export function SourceCode({ code, title = "Source" }: SourceCodeProps) {
  const [open, setOpen] = useState(false);

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Button variant="ghost" size="sm" onClick={() => setOpen(!open)}>
          {open ? "Hide" : "Show"}
        </Button>
      </CardHeader>
      {open && (
        <CardContent className="pt-0">
          <pre className="overflow-x-auto rounded-md bg-muted p-4 text-xs font-mono text-foreground/90 leading-relaxed">
            <code>{code}</code>
          </pre>
        </CardContent>
      )}
    </Card>
  );
}
