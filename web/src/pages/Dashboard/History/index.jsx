import {useEffect, useMemo, useRef, useState} from "react";
import {Empty, Input, Pagination, Segmented, Spin, Tag, message} from "antd";
import {ArrowRight, Box, FileClock, FileMinus2, FilePenLine, FolderCog, Search} from "lucide-react";
import {miscVersionHistoryRequest, miscVersionListRequest} from "@/api/misc.js";
import {showFileSize, showTime} from "@/utils/tool.js";

const PAGE_SIZE = 12;

const operationMeta = {
  'update-file': {label: '写入文件', tone: 'blue', icon: FilePenLine, group: 'write'},
  'delete-file': {label: '删除文件', tone: 'red', icon: FileMinus2, group: 'delete'},
  'delete-file-by-hash': {label: '哈希删除', tone: 'volcano', icon: FileMinus2, group: 'delete'},
  'move-file': {label: '移动文件', tone: 'gold', icon: ArrowRight, group: 'move'},
  'create-directory': {label: '创建目录', tone: 'cyan', icon: FolderCog, group: 'directory'},
  'delete-directory': {label: '删除目录', tone: 'orange', icon: FolderCog, group: 'directory'}
};

const summaryText = counts => {
  if (!counts) return '';
  return `${counts.total} 项 · ${counts.files} 写入 · ${counts.deletions + counts.hash_deletions} 删除 · ${counts.moves} 移动`;
};

const Index = () => {
  const [versions, setVersions] = useState([]);
  const [selected, setSelected] = useState('');
  const [detail, setDetail] = useState(null);
  const [loadingList, setLoadingList] = useState(true);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [versionSearch, setVersionSearch] = useState('');
  const [changeSearch, setChangeSearch] = useState('');
  const [changeGroup, setChangeGroup] = useState('all');
  const [page, setPage] = useState(1);
  const detailScrollRef = useRef(null);
  const [messageApi, contextHolder] = message.useMessage();

  useEffect(() => {
    const load = async () => {
      setLoadingList(true);
      const response = await miscVersionListRequest();
      if (response?.code !== 1) {
        messageApi.error(response?.msg || '无法读取打包历史');
        setLoadingList(false);
        return;
      }
      const next = response.data?.versions || [];
      setVersions(next);
      setLoadingList(false);
      if (next.length > 0) setSelected(next[0].label);
    };
    load();
  }, []);

  useEffect(() => {
    if (!selected) {
      setDetail(null);
      return;
    }
    let current = true;
    setLoadingDetail(true);
    setChangeSearch('');
    setChangeGroup('all');
    miscVersionHistoryRequest(selected).then(response => {
      if (!current) return;
      if (response?.code === 1) setDetail(response.data);
      else {
        setDetail(null);
        messageApi.error(response?.msg || '无法读取版本详情');
      }
    }).finally(() => current && setLoadingDetail(false));
    return () => { current = false; };
  }, [selected]);

  const filteredVersions = useMemo(() => {
    const query = versionSearch.trim().toLowerCase();
    if (!query) return versions;
    return versions.filter(version =>
      version.label.toLowerCase().includes(query) || version.change_logs.toLowerCase().includes(query)
    );
  }, [versions, versionSearch]);

  const pageVersions = filteredVersions.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);

  const filteredChanges = useMemo(() => {
    const query = changeSearch.trim().toLowerCase();
    return (detail?.changes || []).filter(change => {
      const meta = operationMeta[change.operation];
      if (changeGroup !== 'all' && meta?.group !== changeGroup) return false;
      if (!query) return true;
      return [change.path, change.from, change.to, change.hash, change.external_provider]
        .some(value => value?.toLowerCase().includes(query));
    });
  }, [detail, changeGroup, changeSearch]);

  const selectVersion = label => {
    detailScrollRef.current?.scrollTo({top: 0});
    setSelected(label);
    setDetail(null);
  };

  return (
    <div className="flex h-screen min-h-[640px] flex-col overflow-hidden bg-[#f4f8f7] px-6 py-6 text-[#18332d] dark:bg-[#0d1412] dark:text-[#e5efec] xl:px-8">
      {contextHolder}
      <header className="mb-5 flex shrink-0 items-end justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold">打包历史</h1>
          <p className="mt-1 text-sm text-[#687c77] dark:text-[#91a7a1]">查看每个已发布版本的更新日志和真实文件操作。</p>
        </div>
        <div className="text-right text-sm text-[#687c77] dark:text-[#91a7a1]">
          <div className="text-2xl font-semibold text-[#1f6f60] dark:text-[#75c7b5]">{versions.length}</div>
          <div>历史版本</div>
        </div>
      </header>

      <main className="grid min-h-0 flex-1 grid-cols-1 overflow-hidden border border-[#d9e5e1] bg-white dark:border-[#293b37] dark:bg-[#121b19] xl:grid-cols-[360px_minmax(0,1fr)]">
        <aside className="flex min-h-0 flex-col border-b border-[#d9e5e1] dark:border-[#293b37] xl:border-b-0 xl:border-r">
          <div className="border-b border-[#e4ece9] p-4 dark:border-[#293b37]">
            <Input
              allowClear
              prefix={<Search size={15}/>} placeholder="搜索版本号或更新日志"
              value={versionSearch}
              onChange={event => { setVersionSearch(event.target.value); setPage(1); }}
            />
          </div>
          <div className="flex-1 overflow-y-auto">
            {loadingList ? <div className="grid h-48 place-items-center"><Spin/></div> : pageVersions.length === 0 ? (
              <div className="grid h-48 place-items-center"><Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有历史记录"/></div>
            ) : pageVersions.map(version => (
              <button
                type="button" key={version.label} onClick={() => selectVersion(version.label)}
                className={`block w-full border-b border-[#edf2f0] px-4 py-4 text-left transition-colors dark:border-[#22312e] ${selected === version.label ? 'bg-[#eaf5f1] dark:bg-[#17302a]' : 'hover:bg-[#f5f9f8] dark:hover:bg-[#17221f]'}`}
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="font-semibold text-[#215f53] dark:text-[#8bd0c0]">{version.label}</span>
                  <span className="shrink-0 text-xs text-[#81938f]">{showFileSize(version.size)}</span>
                </div>
                <div className="mt-2 line-clamp-2 text-xs leading-5 text-[#687c77] dark:text-[#91a7a1]">{version.change_logs || '未填写更新日志'}</div>
              </button>
            ))}
          </div>
          <div className="border-t border-[#e4ece9] px-3 py-3 dark:border-[#293b37]">
            <Pagination simple current={page} pageSize={PAGE_SIZE} total={filteredVersions.length} onChange={setPage}/>
          </div>
        </aside>

        <section ref={detailScrollRef} className="min-h-0 min-w-0 overflow-y-auto">
          {loadingDetail ? <div className="grid h-full min-h-[480px] place-items-center"><Spin size="large"/></div> : !detail ? (
            <div className="grid h-full min-h-[480px] place-items-center"><Empty description="选择一个版本查看文件变化"/></div>
          ) : (
            <div className="flex min-h-full flex-col">
              <div className="border-b border-[#e4ece9] px-5 py-5 dark:border-[#293b37]">
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div>
                    <div className="flex items-center gap-3"><FileClock size={22}/><h2 className="text-xl font-bold">{detail.label}</h2></div>
                    <p className="mt-2 whitespace-pre-wrap text-sm leading-6 text-[#536a64] dark:text-[#abc0ba]">{detail.change_logs || '未填写更新日志'}</p>
                  </div>
                  <div className="text-right text-xs leading-5 text-[#718681] dark:text-[#91a7a1]">
                    <div>{detail.filename}</div>
                    <div>归档 {showFileSize(detail.archive_size)} · 本次写入 {showFileSize(detail.payload_size)}</div>
                    <div className="font-mono" title={detail.package_hash}>{detail.package_hash.slice(0, 16)}...</div>
                  </div>
                </div>
                <div className="mt-4 flex flex-wrap gap-2">
                  <Tag color="blue">写入 {detail.counts.files}</Tag>
                  <Tag color="red">删除 {detail.counts.deletions + detail.counts.hash_deletions}</Tag>
                  <Tag color="gold">移动 {detail.counts.moves}</Tag>
                  <Tag color="cyan">目录 {detail.counts.directories}</Tag>
                  <span className="self-center text-xs text-[#718681] dark:text-[#91a7a1]">{summaryText(detail.counts)}</span>
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-3 border-b border-[#e4ece9] px-5 py-3 dark:border-[#293b37]">
                <Segmented
                  value={changeGroup} onChange={setChangeGroup}
                  options={[{label: '全部', value: 'all'}, {label: '写入', value: 'write'}, {label: '删除', value: 'delete'}, {label: '移动', value: 'move'}, {label: '目录', value: 'directory'}]}
                />
                <Input allowClear className="min-w-[220px] flex-1" prefix={<Search size={15}/>} placeholder="筛选路径、哈希或下载源" value={changeSearch} onChange={event => setChangeSearch(event.target.value)}/>
                <span className="text-xs text-[#718681] dark:text-[#91a7a1]">显示 {filteredChanges.length} / {detail.changes.length}</span>
              </div>

              <div>
                {filteredChanges.length === 0 ? <div className="grid h-48 place-items-center"><Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有匹配的文件变化"/></div> : filteredChanges.map((change, index) => {
                  const meta = operationMeta[change.operation] || {label: change.operation, tone: 'default', icon: Box};
                  const Icon = meta.icon;
                  const mainPath = change.path || change.to || '';
                  return (
                    <div key={`${change.operation}-${mainPath}-${index}`} className="grid grid-cols-[28px_minmax(0,1fr)_auto] gap-3 border-b border-[#edf2f0] px-5 py-3 dark:border-[#22312e]">
                      <Icon size={17} className="self-center text-[#66807a]"/>
                      <div className="min-w-0">
                        {change.operation === 'move-file' ? (
                          <div className="flex min-w-0 items-center gap-2 font-mono text-sm"><span className="truncate">{change.from}</span><ArrowRight size={14} className="shrink-0"/><span className="truncate">{change.to}</span></div>
                        ) : <div className="truncate font-mono text-sm" title={mainPath}>{mainPath}</div>}
                        <div className="mt-1 flex flex-wrap gap-x-4 text-xs text-[#7a8d88] dark:text-[#91a7a1]">
                          {change.len !== null && <span>{showFileSize(change.len)}</span>}
                          {change.modified !== null && <span>{showTime(change.modified)}</span>}
                          {change.external_provider && <span>来源：{change.external_provider}</span>}
                          {change.hash && <span className="font-mono" title={change.hash}>哈希 {change.hash.slice(0, 16)}...</span>}
                        </div>
                      </div>
                      <Tag className="self-center" color={meta.tone}>{meta.label}</Tag>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </section>
      </main>
    </div>
  );
};

export default Index;
