import { lazy, type ComponentType } from "react";

const loadRecharts = () => import("recharts");
type LazyChartComponent = ComponentType<Record<string, unknown>>;

function lazyRechartsComponent(name: keyof Awaited<ReturnType<typeof loadRecharts>>) {
  return lazy(() =>
    loadRecharts().then((module) => ({
      default: module[name] as LazyChartComponent,
    }))
  );
}

export const Area = lazyRechartsComponent("Area");
export const AreaChart = lazyRechartsComponent("AreaChart");
export const CartesianGrid = lazyRechartsComponent("CartesianGrid");
export const Cell = lazyRechartsComponent("Cell");
export const Label = lazyRechartsComponent("Label");
export const LabelList = lazyRechartsComponent("LabelList");
export const Legend = lazyRechartsComponent("Legend");
export const Line = lazyRechartsComponent("Line");
export const LineChart = lazyRechartsComponent("LineChart");
export const Pie = lazyRechartsComponent("Pie");
export const PieChart = lazyRechartsComponent("PieChart");
export const ReferenceArea = lazyRechartsComponent("ReferenceArea");
export const ReferenceLine = lazyRechartsComponent("ReferenceLine");
export const ResponsiveContainer = lazyRechartsComponent("ResponsiveContainer");
export const Scatter = lazyRechartsComponent("Scatter");
export const ScatterChart = lazyRechartsComponent("ScatterChart");
export const Tooltip = lazyRechartsComponent("Tooltip");
export const XAxis = lazyRechartsComponent("XAxis");
export const YAxis = lazyRechartsComponent("YAxis");
export const ZAxis = lazyRechartsComponent("ZAxis");
