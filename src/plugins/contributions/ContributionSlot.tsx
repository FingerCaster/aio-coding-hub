import { toast } from "sonner";
import { usePluginExecuteCommandMutation } from "../../query/plugins";
import { formatActionFailureToast } from "../../utils/errors";
import { HostRenderedContribution } from "./HostRenderedContribution";
import { useContributionsForSlot } from "./useActiveContributions";
import {
  contributionKey,
  type ContributionCommandHandler,
  type ContributionValues,
  type ContributionSlotProps,
} from "./types";

const EMPTY_VALUES_BY_CONTRIBUTION_KEY: Record<string, ContributionValues> = {};

export function ContributionSlot({
  slotId,
  valuesByContributionKey = EMPTY_VALUES_BY_CONTRIBUTION_KEY,
  onChange,
  onCommand,
  disabled,
}: ContributionSlotProps) {
  const { contributions } = useContributionsForSlot(slotId);
  const executeCommand = usePluginExecuteCommandMutation();

  if (contributions.length === 0) return null;

  function handleCommand(
    values: ContributionValues,
    command: string,
    context: Parameters<ContributionCommandHandler>[1]
  ) {
    if (onCommand) {
      onCommand(command, context);
      return;
    }
    void executeCommand
      .mutateAsync({
        command,
        args: {
          ...context,
          slotId,
          values,
        },
      })
      .catch((error) => {
        toast.error(formatActionFailureToast("执行插件命令", error).toast);
      });
  }

  return (
    <>
      {contributions.map((contribution) => {
        const values = valuesByContributionKey[contributionKey(contribution)] ?? {};

        return (
          <HostRenderedContribution
            key={`${contribution.pluginId}:${contribution.contributionId}`}
            contribution={contribution}
            values={values}
            onChange={(fieldKey, value) => onChange?.(contribution, fieldKey, value)}
            onCommand={(command, context) => handleCommand(values, command, context)}
            disabled={disabled || (!onCommand && executeCommand.isPending)}
          />
        );
      })}
    </>
  );
}
