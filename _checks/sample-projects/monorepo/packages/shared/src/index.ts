export type ServiceConfig = {
  apiOrigin: string;
};

export function createServiceUrl(config: ServiceConfig, path: string) {
  return new URL(path, config.apiOrigin).toString();
}
