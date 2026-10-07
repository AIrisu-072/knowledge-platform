import { RuntimeFailure, type RuntimeAdapter, type RuntimeCapabilities } from './contract';

const UNAVAILABLE: RuntimeCapabilities = Object.freeze({
  localResources: 'unavailable', nativeDirectoryPicker: 'unavailable', managedWorkspace: 'unavailable', multiWindow: false, sidecar: false,
});

const refuse = async (): Promise<never> => { throw new RuntimeFailure('unavailable', 'unsupported_platform'); };

/**
 * Browser runtime: local folders and managed roots are explicitly unavailable
 * (the File System Access adapter is future/low priority). It never fabricates
 * a managed root or a folder binding.
 */
export const browserRuntime: RuntimeAdapter = Object.freeze({
  kind: 'browser' as const,
  capabilities: async () => UNAVAILABLE,
  workspace: Object.freeze({ listWorkspaces: refuse, createLocalWorkspace: refuse, renameWorkspace: refuse, recoverWorkspace: refuse }),
  dialog: Object.freeze({ chooseDirectory: refuse }),
  resources: Object.freeze({
    attachDirectory: refuse, detachDirectory: refuse, listEntries: refuse, openRead: refuse, readFile: refuse, closeRead: refuse, createFile: refuse,
  }),
});
