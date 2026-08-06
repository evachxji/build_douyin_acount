import { useEffect, useRef } from "react";

interface ResizeHandleProps {
  direction: "vertical" | "horizontal";
  onResize: (delta: number) => void;
}

/** 拖拽分隔条：vertical 用于左右栏宽度，horizontal 用于底部栏高度。 */
export function ResizeHandle({ direction, onResize }: ResizeHandleProps) {
  const lastRef = useRef<number | null>(null);
  const onResizeRef = useRef(onResize);
  onResizeRef.current = onResize;

  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (lastRef.current === null) return;
      const pos = direction === "vertical" ? e.clientX : e.clientY;
      onResizeRef.current(pos - lastRef.current);
      lastRef.current = pos;
    };
    const stop = () => {
      lastRef.current = null;
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
      window.removeEventListener("blur", stop);
    };
  }, [direction]);

  const horizontal = direction === "horizontal";
  return (
    <div
      role="separator"
      aria-orientation={direction}
      onMouseDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        lastRef.current = horizontal ? e.clientY : e.clientX;
      }}
      className={
        horizontal
          ? "z-10 h-1.5 shrink-0 cursor-row-resize bg-transparent transition-colors hover:bg-accent/40 active:bg-accent/50"
          : "z-10 w-1.5 shrink-0 cursor-col-resize bg-transparent transition-colors hover:bg-accent/40 active:bg-accent/50"
      }
    />
  );
}
