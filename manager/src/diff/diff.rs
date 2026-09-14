//! 目录差异对比

use std::collections::HashMap;
use std::collections::LinkedList;
use std::fmt::Debug;
use std::fmt::Write;
use std::ops::Deref;
use std::fmt::Display;

use crate::core::data::version_meta::FileChange;
use crate::diff::abstract_file::AbstractFile;
use crate::diff::abstract_file::BorrowIntoIterator;
use crate::core::rule_filter::RuleFilter;
use crate::utility::unix_timestamp::UnixTimestampExt;
use crate::utility::vec_ext::VecRemoveIf;

const OP_FULL_ADDED_FOLDER: &str = "创建目录: ";
const OP_FULL_ADDED_FILE: &str   = "更新文件: ";
const OP_FULL_MODIFIED_FILE: &str   = "修改文件: ";
const OP_FULL_MISSING_FOLDER: &str = "删除目录: ";
const OP_FULL_MISSING_FILE: &str   = "删除文件: ";
const OP_FULL_MOVE_FILE: &str     = "移动文件: ";
const OP_SHORT_ADDED_FOLDER: &str = OP_FULL_ADDED_FOLDER;
const OP_SHORT_ADDED_FILE: &str   = OP_FULL_ADDED_FILE;
const OP_SHORT_MODIFIED_FILE: &str   = OP_FULL_MODIFIED_FILE;
const OP_SHORT_MISSING_FOLDER: &str = OP_FULL_MISSING_FOLDER;
const OP_SHORT_MISSING_FILE: &str   = OP_FULL_MISSING_FILE;
const OP_SHORT_MOVE_FILE: &str     = OP_FULL_MOVE_FILE;

/// 代表一组文件差异
pub struct Diff<N: AbstractFile, O: AbstractFile> {
    pub added_folders: Vec<N>,
    pub added_files: Vec<N>,
    pub modified_files: Vec<N>,
    pub missing_folders: Vec<O>,
    pub missing_files: Vec<O>,
    pub mtime_fix: Vec<(N, O)>,
    pub renamed_files: Vec<(O, N)>,
    excluding_filter: RuleFilter,
}

impl<N: AbstractFile, O: AbstractFile> Diff<N, O> {
    /// 执行目录比较
    pub fn diff(newer: &N, older: &O, filter_rules: Option<&Vec<String>>) -> Self {
        let mut result = Diff {
            added_folders: Vec::new(),
            added_files: Vec::new(),
            modified_files: Vec::new(),
            missing_folders: Vec::new(),
            missing_files: Vec::new(),
            mtime_fix: Vec::new(),
            renamed_files: Vec::new(),
            excluding_filter: match filter_rules {
                Some(filter_rules) => RuleFilter::from_rules(filter_rules.iter()),
                None => RuleFilter::new(),
            },
        };

        result.find_added(newer, older);
        result.find_missing(newer, older);
        result.find_modified(newer, older);
        result.detect_file_movings(newer, older);
        
        result
    }

    /// 有没有不同
    pub fn has_diff(&self) -> bool {
        !self.added_folders.is_empty() ||
        !self.added_files.is_empty() ||
        !self.modified_files.is_empty() ||
        !self.missing_folders.is_empty() ||
        !self.missing_files.is_empty() ||
        !self.renamed_files.is_empty()
    }

    /// 寻找新增的文件
    fn find_added(&mut self, newer: &N, older: &O) {
        assert!(newer.is_dir());
        assert!(older.is_dir());

        for n in newer.files().iter() {
            if !self.is_visible(n.path().deref()) {
                continue;
            }

            let find = older.find(&n.name());

            match find {
                Some(o) => {
                    match (n.is_dir(), o.is_dir()) {
                        // 两边都是目录则进入递归
                        (true, true) => self.find_added(&n, &o),

                        // 两边类型不一样，则会先删除后添加
                        (true, false) => self.mark_as_added(&n),
                        (false, true) => self.mark_as_added(&n),

                        // 两边都是文件，跳过，会由文件修改检查函数来处理此情况
                        (false, false) => (),
                    }
                },

                // 在旧目录里找不到，此时肯定是新增的文件
                None => self.mark_as_added(&n),
            }
        }
    }

    /// 寻找删除的文件
    fn find_missing(&mut self, newer: &N, older: &O) {
        assert!(newer.is_dir());
        assert!(older.is_dir());

        for o in older.files().iter() {
            let found = match newer.find(&o.name()) {
                Some(o) => if self.is_visible(o.path().deref()) { Some(o) } else { None },
                None => None,
            };

            match found {
                Some(n) => match (o.is_dir(), n.is_dir()) {
                    // 两边都是目录就进入递归
                    (true, true) => self.find_missing(&n, &o),

                    // 两边文件类型不一样，就先删除再添加
                    (true, false) => self.mark_as_missing(&o),
                    (false, true) => self.mark_as_missing(&o),

                    // 两边都是文件，跳过，会由文件修改检查函数来处理此情况
                    (false, false) => (),
                },

                // 在新目录里找不到，此时肯定是被删除的文件
                None => self.mark_as_missing(&o),
            }
        }
    }

    /// 寻找修改的文件
    fn find_modified(&mut self, newer: &N, older: &O) {
        assert!(newer.is_dir());
        assert!(older.is_dir());

        for n in newer.files().iter() {
            if !self.is_visible(n.path().deref()) {
                continue;
            }

            let find = older.find(&n.name());

            match find {
                Some(o) => {
                    match (n.is_dir(), o.is_dir()) {
                        // 两边都是目录则进入递归
                        (true, true) => self.find_modified(&n, &o),

                        // 两边类型不一样，跳过，会由文件新增和文件删除检测函数来处理此情况
                        (true, false) => (),
                        (false, true) => (),

                        // 两边都是文件，则对比文件，如果不同，视为修改过的文件
                        (false, false) => {
                            if !Self::compare_file_mtime(&n, &o) {
                                if !Self::compare_file_hash(&n, &o) {
                                    self.mark_as_modified(&n)
                                } else {
                                    self.mtime_fix.push((n.to_owned(), o.to_owned()));
                                }
                            }
                        },
                    }
                },

                // 在旧目录里找不到，此情况已由文件新增检测函数处理过
                None => (),
            }
        }
    }

    /// 将一个文件或者目录标记成删除
    fn mark_as_missing(&mut self, file: &O) {
        if file.is_dir() {
            for f in file.files().iter() {
                self.mark_as_missing(&f);
            }

            self.missing_folders.push(file.to_owned());
        } else {
            self.missing_files.push(file.to_owned());
        }
    }

    /// 将一个文件或者目录标记为新增
    fn mark_as_added(&mut self, file: &N) {
        if !self.is_visible(&file.path()) {
            return;
        }

        if file.is_dir() {
            self.added_folders.push(file.clone());

            for f in file.files().iter() {
                self.mark_as_added(&f);
            }
        } else {
            self.added_files.push(file.to_owned());
        }
    }

    /// 将一个文件标记为修改过的文件，目录不行
    fn mark_as_modified(&mut self, file: &N) {
        if !self.is_visible(&file.path()) {
            return;
        }

        assert!(!file.is_dir());

        self.modified_files.push(file.to_owned());
    }
    
    /// 比较两个文件的修改时间
    fn compare_file_mtime(a: &N, b: &O) -> bool {
        let ta = a.modified().as_unix_seconds();
        let tb = b.modified().as_unix_seconds();

        ta == tb
    }
    
    /// 比较两个文件的校验
    fn compare_file_hash(a: &N, b: &O) -> bool {
        a.hash().deref() == b.hash().deref()
    }

    /// 检查一个文件要不要被忽略
    fn is_visible(&self, path: &str) -> bool {
        !self.excluding_filter.test_any(path, false)
    }
    
    /// 检测文件移动操作
    fn detect_file_movings(&mut self, _newer: &N, _older: &O) {
        let mut old_hashes = HashMap::<String, Vec<O>>::new();
        let mut new_hashes = HashMap::<String, Vec<N>>::new();
        
        // 建立hash和路径之间的映射
        for missing_file in &self.missing_files {
            let list = old_hashes.get_mut(missing_file.hash().deref());
            
            match list {
                Some(hashes) => {
                    hashes.push(missing_file.to_owned());
                },
                None => {
                    let mut list = Vec::<O>::new();
                    list.push(missing_file.to_owned());
                    old_hashes.insert(missing_file.hash().to_owned(), list);
                },
            }
        }
        
        for added_file in &self.added_files {
            let list = new_hashes.get_mut(added_file.hash().deref());
            
            match list {
                Some(hashes) => {
                    hashes.push(added_file.to_owned());
                },
                None => {
                    let mut list = Vec::<N>::new();
                    list.push(added_file.to_owned());
                    new_hashes.insert(added_file.hash().to_owned(), list);
                },
            }
        }
        
        // for missing_file in &self.missing_files {
        //     println!("missing_file: {} ({})", missing_file.path().deref(), missing_file.hash().deref());
        // }
        
        // for added_file in &self.added_files {
        //     println!("added_file: {} ({})", added_file.path().deref(), added_file.hash().deref());
        // }
        
        // for e in &old_hashes {
        //     for path in e.1 {
        //         println!("old_hashes: {} => {} ({})", e.0, path.path().deref(), e.1.len());
        //     }
        // }
        
        // for e in &new_hashes {
        //     for path in e.1 {
        //         println!("new_hashes: {} => {} ({})", e.0, path.path().deref(), e.1.len());
        //     }
        // }
        
        // 寻找一对一的hash，这些文件就是发生了移动的文件
        for (hash, olds) in old_hashes {
            if let Some(news) = new_hashes.get(&hash) {
                if olds.len() == 1 && news.len() == 1 {
                    let o = olds.get(0).unwrap();
                    let n = news.get(0).unwrap();
                    
                    self.renamed_files.push((o.to_owned(), n.to_owned()));
                    
                    self.missing_files.remove_if(|e| e.path().deref() == o.path().deref());
                    self.added_files.remove_if(|e| e.path().deref() == n.path().deref());
                }
            }
        }
    }

    /// 将一个`diff`对象转换成文件变动列表
    pub fn to_file_changes(&self) -> LinkedList<FileChange> {
        let mut changes = LinkedList::new();
    
        for f in &self.missing_files {
            changes.push_back(FileChange::DeleteFile { 
                path: f.path().to_owned() 
            })
        }
    
        for f in &self.added_folders {
            changes.push_back(FileChange::CreateFolder { 
                path: f.path().to_owned() 
            })
        }
    
        for f in &self.renamed_files {
            changes.push_back(FileChange::MoveFile {
                from: f.0.path().to_owned(), 
                to: f.1.path().to_owned()
            })
        }
    
        for f in &self.added_files {
            changes.push_back(FileChange::UpdateFile { 
                path: f.path().to_owned(), 
                hash: f.hash().to_owned(), 
                len: f.len(), 
                modified: f.modified(), 
                offset: 0, // 此时offset是空的，需要由TarWriter去填充
                external_source: None,
            })
        }

        for f in &self.modified_files {
            changes.push_back(FileChange::UpdateFile { 
                path: f.path().to_owned(), 
                hash: f.hash().to_owned(), 
                len: f.len(), 
                modified: f.modified(), 
                offset: 0, // 此时offset是空的，需要由TarWriter去填充
                external_source: None,
            })
        }
    
        for f in &self.missing_folders {
            changes.push_back(FileChange::DeleteFolder { 
                path: f.path().to_owned() 
            })
        }
    
        changes
    }
}

impl<N: AbstractFile, O: AbstractFile> Display for Diff<N, O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("Diff ({}{}, {}{}, {}{}, {}{}, {}{}, {}{})",
            OP_SHORT_ADDED_FOLDER, self.added_folders.len(),
            OP_SHORT_ADDED_FILE, self.added_files.len(),
            OP_SHORT_MODIFIED_FILE, self.modified_files.len(),
            OP_SHORT_MISSING_FOLDER, self.missing_folders.len(),
            OP_SHORT_MISSING_FILE, self.missing_files.len(),
            OP_SHORT_MOVE_FILE, self.renamed_files.len(),
        ))
    }
}

macro_rules! printn {
    ($flag:ident, $fmt:ident) => {
        if $flag {
            $fmt.write_char('\n')?;
        }

        $flag = true;
    };
}

impl<N: AbstractFile, O: AbstractFile> Debug for Diff<N, O> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut need_newline = false;

        for f in &self.missing_files {
            printn!(need_newline, fmt);
            fmt.write_str(&format!("{}{}", OP_FULL_MISSING_FILE, f.path().deref()))?;
        }
    
        for f in &self.added_folders {
            printn!(need_newline, fmt);
            fmt.write_str(&format!("{}{}", OP_FULL_ADDED_FOLDER, f.path().deref()))?;
        }
    
        for (n, o) in &self.renamed_files {
            printn!(need_newline, fmt);
            fmt.write_str(&format!("{}{} -> {}", OP_FULL_MOVE_FILE, n.path().deref(), o.path().deref()))?;
        }
    
        for f in &self.added_files {
            printn!(need_newline, fmt);
            fmt.write_str(&format!("{}{}", OP_FULL_ADDED_FILE, f.path().deref()))?;
        }

        for f in &self.modified_files {
            printn!(need_newline, fmt);
            fmt.write_str(&format!("{}{}", OP_FULL_MODIFIED_FILE, f.path().deref()))?;
        }
    
        for f in &self.missing_folders {
            printn!(need_newline, fmt);
            fmt.write_str(&format!("{}{}", OP_FULL_MISSING_FOLDER, f.path().deref()))?;
        }

        Ok(())
    }
}
