import { ArrowLeft, X } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { CodexRetryGatewayManager } from "../components/cli-manager/tabs/CodexRetryGatewayManager";
import { Button } from "../ui/Button";
import { PageHeader } from "../ui/PageHeader";

export function CodexGatewayPage() {
  const navigate = useNavigate();

  return (
    <div className="flex h-full flex-col gap-6 overflow-hidden">
      <PageHeader
        title="Codex 外部网关"
        subtitle="受管实例状态与本地桥接安全边界"
        actions={
          <>
            <Button type="button" variant="secondary" size="sm" onClick={() => navigate(-1)}>
              <ArrowLeft className="h-4 w-4" aria-hidden="true" />
              返回
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => navigate("/cli-manager")}
            >
              <X className="h-4 w-4" aria-hidden="true" />
              退出
            </Button>
          </>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto scrollbar-overlay pb-2">
        <CodexRetryGatewayManager showDetailsFrame={true} />
      </div>
    </div>
  );
}
