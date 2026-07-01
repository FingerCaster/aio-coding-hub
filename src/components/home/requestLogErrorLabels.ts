import { getGatewayErrorShortLabel } from "../../constants/gatewayErrorCodes";

export function getErrorCodeLabel(errorCode: string) {
  return getGatewayErrorShortLabel(errorCode);
}
