const reasons=new Map([
 ['projected total elapsed time exceeds wall-time budget','projected-wall-time-exceeds-budget'],
 ['projected total elapsed time exceeds remaining per-run deadline','projected-time-exceeds-deadline'],
 ['projected disk growth would consume disk reserve','projected-disk-exhausts-reserve'],
 ['projected process-tree RSS would consume available-memory reserve','projected-memory-exhausts-reserve'],
 ['projected process-tree RSS exceeds budget','projected-rss-exceeds-budget'],
 ['current free disk is below disk reserve','current-disk-below-reserve'],
 ['current available memory is below reserve','current-memory-below-reserve'],
 ['current process-tree RSS exceeds budget','current-rss-exceeds-budget'],
 ['per-run deadline has elapsed','deadline-elapsed'],
 ['previous report corpus, code or runtime fingerprint differs','previous-fingerprint-mismatch'],
 ['complete process-tree RSS coverage is required','incomplete-rss-coverage'],
 ['database filesystem capacity must be measured or explicitly verified to share storage filesystem','database-capacity-unverified'],
 ['resource observation is required','resource-observation-missing'],
 ...['diskFreeBytes','storageDiskFreeBytes','databaseDiskFreeBytes','availableMemoryBytes','rssBytes','storageBytes','databaseBytes'].map(field=>[`missing or invalid measured ${field}`,field==='rssBytes'?'missing-rss-measurement':'missing-resource-measurement']),
]);
const numeric=(value,keys)=>Object.fromEntries(keys.map(key=>[key,typeof value?.[key]==='number'&&Number.isFinite(value[key])&&value[key]>=0&&value[key]<=Number.MAX_SAFE_INTEGER?value[key]:null]));
export function summarizeAdmission(report){
 const admission=report?.admission;
 if(!['ADMITTED','NOT_ADMITTED'].includes(admission?.status))return null;
 return {status:admission.status,reasonCodes:[...new Set((Array.isArray(admission.reasons)?admission.reasons:[]).slice(0,32).map(reason=>reasons.get(reason)??'invalid-admission-evidence'))],
 projection:admission.projection?numeric(admission.projection,['totalElapsedMs','diskGrowthBytes','peakRssBytes','multiplier']):null,
 budgets:numeric(report.plan?.budgets,['maxWallTimeMs','maxRssBytes','diskReserveBytes','minAvailableMemoryBytes']),
 current:numeric(report.observations?.at(-1),['diskFreeBytes','storageDiskFreeBytes','databaseDiskFreeBytes','availableMemoryBytes','rssBytes'])};
}
