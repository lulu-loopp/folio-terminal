globalThis.folioPolicyReady = Promise.resolve(true);

globalThis.folioTargetTabId = async targetId => {
  const targets = await chrome.debugger.getTargets();
  const target = targets.find(candidate => candidate.id === targetId);
  if (!target || !Number.isInteger(target.tabId)) {
    throw new Error(`CDP target ${targetId} has no Chrome tab id`);
  }
  return target.tabId;
};

globalThis.folioSetWebSocketPolicy = async (targetId, ruleBase, deny) => {
  const tabId = await globalThis.folioTargetTabId(targetId);
  const removeRuleIds = [ruleBase, ruleBase + 1];
  const addRules = deny ? [
    {
      id: ruleBase,
      priority: 1,
      action: {type: "block"},
      condition: {
        urlFilter: "|ws://",
        resourceTypes: ["websocket"],
        tabIds: [tabId]
      }
    },
    {
      id: ruleBase + 1,
      priority: 1,
      action: {type: "block"},
      condition: {
        urlFilter: "|wss://",
        resourceTypes: ["websocket"],
        tabIds: [tabId]
      }
    }
  ] : [];
  await chrome.declarativeNetRequest.updateSessionRules({removeRuleIds, addRules});
  return {tabId, denied: deny};
};
