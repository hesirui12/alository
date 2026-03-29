//! 条形码商品信息导入系统
//!
//! 本程序用于通过条形码扫描或手动输入的方式，将商品信息导入到 SQLite 数据库中。
//! 支持通过 API 自动查询商品信息，也支持手动录入。

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 数据库文件名
const DB_FILE: &str = "product.db";
/// 配置文件名
const CONFIG_FILE: &str = "config.json";
/// 条形码查询 API 基础地址
const API_BASE_URL: &str = "https://apione.apibyte.cn/api/barcode";

/// 获取数据库文件路径
///
/// 返回程序同级目录下的数据库文件路径
fn get_db_path() -> Result<std::path::PathBuf> {
    let exe_path = std::env::current_exe()
        .context("无法获取程序路径")?;
    let exe_dir = exe_path.parent()
        .context("无法获取程序所在目录")?;
    Ok(exe_dir.join(DB_FILE))
}

/// 获取配置文件路径
///
/// 返回程序同级目录下的配置文件路径
fn get_config_path() -> Result<std::path::PathBuf> {
    let exe_path = std::env::current_exe()
        .context("无法获取程序路径")?;
    let exe_dir = exe_path.parent()
        .context("无法获取程序所在目录")?;
    Ok(exe_dir.join(CONFIG_FILE))
}

/// 商品数据结构
///
/// 存储商品的基本信息，包括条形码、名称、分类、价格、库存等
#[derive(Debug, Serialize, Deserialize)]
struct Product {
    /// 商品条形码，唯一标识
    barcode: String,
    /// 商品名称
    name: String,
    /// 商品分类
    category: String,
    /// 商品价格
    price: f64,
    /// 库存数量
    stock: i32,
    /// 计量单位（个、瓶、箱等）
    unit: String,
}

/// 程序配置
///
/// 存储用户的配置信息，如 API Key
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    /// API 密钥，用于查询商品信息
    api_key: Option<String>,
}

impl Config {
    /// 创建默认配置
    fn new() -> Self {
        Config { api_key: None }
    }
    
    /// 从配置文件加载配置
    ///
    /// 如果配置文件不存在，则返回默认配置
    fn load() -> Result<Self> {
        let config_path = get_config_path()?;
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Config::new())
        }
    }
    
    /// 保存配置到文件
    fn save(&self) -> Result<()> {
        let config_path = get_config_path()?;
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }
    
    /// 设置 API Key
    fn set_api_key(&mut self, key: String) {
        self.api_key = Some(key);
    }
    
    /// 获取 API Key
    fn get_api_key(&self) -> Option<&String> {
        self.api_key.as_ref()
    }
}

/// API 响应结构
///
/// 条形码查询 API 的响应格式
#[derive(Debug, Deserialize)]
struct ApiResponse {
    /// 响应状态码，200 表示成功
    code: i32,
    /// 响应消息
    msg: String,
    /// 响应数据
    data: Option<ProductData>,
}

/// API 返回的商品数据
///
/// 包含商品的详细信息
#[derive(Debug, Deserialize)]
struct ProductData {
    /// 商品名称
    #[serde(rename = "goodsName")]
    goods_name: Option<String>,
    /// 商品分类
    #[serde(rename = "category")]
    category: Option<String>,
    /// 商品价格
    #[serde(rename = "price")]
    price: Option<String>,
    /// 品牌
    #[serde(rename = "brand")]
    brand: Option<String>,
    /// 生产公司
    #[serde(rename = "company")]
    company: Option<String>,
    /// 规格
    #[serde(rename = "specification")]
    specification: Option<String>,
    /// 商品图片 URL
    #[serde(rename = "image")]
    image: Option<String>,
    /// 是否找到商品
    #[serde(rename = "found")]
    found: Option<bool>,
}

/// 数据库操作结构
///
/// 封装 SQLite 数据库的连接和操作
struct Database {
    /// 数据库连接
    conn: Connection,
}

impl Database {
    /// 创建数据库连接并初始化表结构
    ///
    /// # 参数
    /// * `db_path` - 数据库文件路径
    ///
    /// # 返回
    /// * `Result<Self>` - 成功返回 Database 实例，失败返回错误
    fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("无法打开数据库: {}", db_path.display()))?;
        
        // 创建商品表（如果不存在）
        conn.execute(
            "CREATE TABLE IF NOT EXISTS products (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                barcode TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                category TEXT,
                price REAL,
                stock INTEGER DEFAULT 0,
                unit TEXT DEFAULT '个',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        
        Ok(Database { conn })
    }
    
    /// 插入或更新商品
    ///
    /// 如果条形码已存在，则更新商品信息；否则插入新商品
    ///
    /// # 参数
    /// * `product` - 要插入或更新的商品
    fn insert_or_update_product(&self, product: &Product) -> Result<()> {
        self.conn.execute(
            "INSERT INTO products (barcode, name, category, price, stock, unit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(barcode) DO UPDATE SET
             name = excluded.name,
             category = excluded.category,
             price = excluded.price,
             stock = excluded.stock,
             unit = excluded.unit,
             updated_at = CURRENT_TIMESTAMP",
            params![
                product.barcode,
                product.name,
                product.category,
                product.price,
                product.stock,
                product.unit
            ],
        )?;
        
        Ok(())
    }
    
    /// 根据条形码查询商品
    ///
    /// # 参数
    /// * `barcode` - 商品条形码
    ///
    /// # 返回
    /// * `Result<Option<Product>>` - 找到返回商品，未找到返回 None
    fn search_product(&self, barcode: &str) -> Result<Option<Product>> {
        let mut stmt = self.conn.prepare(
            "SELECT barcode, name, category, price, stock, unit FROM products WHERE barcode = ?1"
        )?;
        
        let result = stmt.query_row([barcode], |row| {
            Ok(Product {
                barcode: row.get(0)?,
                name: row.get(1)?,
                category: row.get(2)?,
                price: row.get(3)?,
                stock: row.get(4)?,
                unit: row.get(5)?,
            })
        });
        
        match result {
            Ok(product) => Ok(Some(product)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    
    /// 获取所有商品列表
    ///
    /// 按创建时间倒序排列
    ///
    /// # 返回
    /// * `Result<Vec<Product>>` - 商品列表
    fn list_products(&self) -> Result<Vec<Product>> {
        let mut stmt = self.conn.prepare(
            "SELECT barcode, name, category, price, stock, unit FROM products ORDER BY created_at DESC"
        )?;
        
        let products = stmt.query_map([], |row| {
            Ok(Product {
                barcode: row.get(0)?,
                name: row.get(1)?,
                category: row.get(2)?,
                price: row.get(3)?,
                stock: row.get(4)?,
                unit: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
        
        Ok(products)
    }
}

/// 读取用户输入
///
/// # 参数
/// * `prompt` - 提示信息
///
/// # 返回
/// * `Result<String>` - 用户输入的字符串
fn read_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    
    Ok(input.trim().to_string())
}

/// 配置 API Key
///
/// 引导用户输入并保存 API Key
///
/// # 参数
/// * `config` - 配置对象
fn configure_api_key(config: &mut Config) -> Result<()> {
    println!("\n=== API Key 配置 ===");
    println!("获取 API Key: https://www.apibyte.cn/login");
    println!("当前状态: {}", 
        if config.get_api_key().is_some() { "已配置" } else { "未配置" }
    );
    
    let key = read_line("请输入 API Key (留空则保持不变): ")?;
    
    if !key.is_empty() {
        config.set_api_key(key);
        config.save()?;
        println!("✓ API Key 已保存");
    } else {
        println!("保持当前配置");
    }
    
    Ok(())
}

/// 调用条形码查询 API
///
/// # 参数
/// * `barcode` - 商品条形码
/// * `api_key` - API 密钥
///
/// # 返回
/// * `Result<Option<ProductData>>` - API 返回的商品数据
async fn query_barcode_api(barcode: &str, api_key: &str) -> Result<Option<ProductData>> {
    let client = reqwest::Client::new();
    let url = format!("{}?key={}&barcode={}", API_BASE_URL, api_key, barcode);
    
    let response = client
        .get(&url)
        .send()
        .await
        .context("API 请求失败")?;
    
    let api_response: ApiResponse = response
        .json()
        .await
        .context("解析 API 响应失败")?;
    
    if api_response.code == 200 {
        Ok(api_response.data)
    } else {
        println!("API 错误: {}", api_response.msg);
        Ok(None)
    }
}

/// 自动导入商品（通过 API 查询）
///
/// 使用条形码查询 API 获取商品信息并导入数据库
///
/// # 参数
/// * `db` - 数据库实例
/// * `barcode` - 商品条形码
/// * `config` - 配置对象
///
/// # 返回
/// * `Result<bool>` - 导入成功返回 true，失败返回 false
async fn auto_import_product(db: &Database, barcode: &str, config: &Config) -> Result<bool> {
    let api_key = match config.get_api_key() {
        Some(key) => key,
        None => {
            println!("未配置 API Key，无法自动查询");
            return Ok(false);
        }
    };
    
    println!("正在通过 API 查询商品信息...");
    
    match query_barcode_api(barcode, api_key).await {
        Ok(Some(data)) => {
            // 检查API是否找到商品
            if let Some(false) = data.found {
                println!("API 未找到该商品信息");
                return Ok(false);
            }
            
            // 获取商品信息，如果API返回空则提示手动输入
            let name = match &data.goods_name {
                Some(n) if !n.is_empty() => n.clone(),
                _ => {
                    println!("API 未返回商品名称，需要手动输入");
                    let input = read_line("请输入商品名称: ")?;
                    if input.is_empty() {
                        println!("商品名称不能为空!");
                        return Ok(false);
                    }
                    input
                }
            };
            
            let category = data.category.unwrap_or_else(|| "其他".to_string());
            let price = data.price.and_then(|p| p.parse().ok()).unwrap_or(0.0);
            
            println!("\n查询到商品信息:");
            println!("  条形码: {}", barcode);
            println!("  名称: {}", name);
            println!("  分类: {}", category);
            println!("  价格: ¢{:.2}", price);
            
            if let Some(brand) = &data.brand {
                if !brand.is_empty() {
                    println!("  品牌: {}", brand);
                }
            }
            if let Some(company) = &data.company {
                if !company.is_empty() {
                    println!("  公司: {}", company);
                }
            }
            if let Some(spec) = &data.specification {
                if !spec.is_empty() {
                    println!("  规格: {}", spec);
                }
            }
            if let Some(image) = &data.image {
                if !image.is_empty() {
                    println!("  图片: {}", image);
                }
            }
            
            let confirm = read_line("\n是否使用该信息导入? (y/n) [默认: y]: ")?;
            if confirm.is_empty() || confirm.to_lowercase() == "y" || confirm.to_lowercase() == "yes" {
                let stock = read_line("请输入库存数量 [默认: 0]: ")?
                    .parse()
                    .unwrap_or(0);
                
                let unit = read_line("请输入单位 [默认: 个]: ")?;
                let unit = if unit.is_empty() { "个".to_string() } else { unit };
                
                let product = Product {
                    barcode: barcode.to_string(),
                    name,
                    category,
                    price,
                    stock,
                    unit,
                };
                
                db.insert_or_update_product(&product)?;
                println!("✓ 商品 '{}' 已成功导入数据库!", barcode);
                return Ok(true);
            } else {
                println!("已取消自动导入");
                return Ok(false);
            }
        }
        Ok(None) => {
            println!("API 未返回该商品信息");
            return Ok(false);
        }
        Err(e) => {
            println!("API 查询失败: {}", e);
            return Ok(false);
        }
    }
}

/// 手动录入商品
///
/// 引导用户手动输入商品信息
///
/// # 参数
/// * `db` - 数据库实例
/// * `barcode` - 商品条形码
fn manual_import_product(db: &Database, barcode: &str) -> Result<()> {
    println!("\n=== 手动录入商品 ===");
    println!("条形码: {}", barcode);
    
    let name = read_line("请输入商品名称: ")?;
    if name.is_empty() {
        println!("商品名称不能为空!");
        return Ok(());
    }
    
    let category = read_line("请输入分类 [默认: 其他]: ")?;
    let category = if category.is_empty() { "其他".to_string() } else { category };
    
    let price = read_line("请输入价格 [默认: 0]: ")?
        .parse()
        .unwrap_or(0.0);
    
    let stock = read_line("请输入库存数量 [默认: 0]: ")?
        .parse()
        .unwrap_or(0);
    
    let unit = read_line("请输入单位 [默认: 个]: ")?;
    let unit = if unit.is_empty() { "个".to_string() } else { unit };
    
    let product = Product {
        barcode: barcode.to_string(),
        name,
        category,
        price,
        stock,
        unit,
    };
    
    db.insert_or_update_product(&product)?;
    println!("✓ 商品 '{}' 已成功导入数据库!", barcode);
    
    Ok(())
}

/// 扫描条形码录入流程
///
/// 完整的条形码扫描和录入流程，包括：
/// 1. 读取条形码
/// 2. 查询数据库
/// 3. 选择录入方式（自动/手动）
///
/// # 参数
/// * `db` - 数据库实例
/// * `config` - 配置对象
async fn scan_barcode_flow(db: &Database, config: &Config) -> Result<()> {
    let barcode = read_line("\n请扫描或输入条形码: ")?;
    
    if barcode.is_empty() {
        println!("条形码不能为空!");
        return Ok(());
    }
    
    // 先查询数据库
    match db.search_product(&barcode) {
        Ok(Some(product)) => {
            println!("\n该商品已存在于数据库中:");
            display_product(&product);
            
            let update = read_line("\n是否更新该商品? (y/n) [默认: n]: ")?;
            if update.to_lowercase() == "y" || update.to_lowercase() == "yes" {
                manual_import_product(db, &barcode)?;
            } else {
                println!("已取消更新");
            }
        }
        Ok(None) => {
            // 商品不存在，开始录入流程
            println!("\n未找到该商品，开始录入流程...");
            
            println!("\n请选择录入方式:");
            println!("1. 自动查询 (使用 API)");
            println!("2. 手动录入");
            println!();
            
            let choice = read_line("请输入选项 (1-2): ")?;
            
            match choice.as_str() {
                "1" => {
                    let success = auto_import_product(db, &barcode, config).await?;
                    if !success {
                        let manual = read_line("\n自动查询失败，是否改为手动录入? (y/n) [默认: y]: ")?;
                        if manual.is_empty() || manual.to_lowercase() == "y" {
                            manual_import_product(db, &barcode)?;
                        }
                    }
                }
                "2" => {
                    manual_import_product(db, &barcode)?;
                }
                _ => {
                    println!("无效的选项");
                }
            }
        }
        Err(e) => {
            println!("查询失败: {}", e);
        }
    }
    
    Ok(())
}

/// 显示商品信息
///
/// # 参数
/// * `product` - 商品实例
fn display_product(product: &Product) {
    println!("  条形码: {}", product.barcode);
    println!("  名称: {}", product.name);
    println!("  分类: {}", product.category);
    println!("  价格: ¢{:.2}", product.price);
    println!("  库存: {} {}", product.stock, product.unit);
}

/// 列出所有商品
///
/// # 参数
/// * `db` - 数据库实例
fn list_all_products(db: &Database) -> Result<()> {
    let products = db.list_products()?;
    
    if products.is_empty() {
        println!("\n数据库中没有商品");
    } else {
        println!("\n=== 商品列表 ===");
        println!("{:<20} {:<30} {:<12} {:>10} {:>8}", "条形码", "商品名称", "分类", "价格", "库存");
        println!("{}", "-".repeat(90));
        for product in products {
            println!("{:<20} {:<30} {:<12} ¢{:>8.2} {:>8}", 
                if product.barcode.len() > 18 { &product.barcode[..18] } else { &product.barcode },
                if product.name.len() > 28 { &product.name[..28] } else { &product.name },
                if product.category.len() > 10 { &product.category[..10] } else { &product.category },
                product.price, 
                product.stock
            );
        }
    }
    
    Ok(())
}

/// 程序入口
///
/// 初始化数据库和配置，进入主菜单循环
#[tokio::main]
async fn main() -> Result<()> {
    let db_path = get_db_path()?;
    let db = Database::new(&db_path)?;
    let mut config = Config::load()?;
    
    println!("=================================");
    println!("   条形码商品信息导入系统");
    println!("=================================");
    
    loop {
        println!("\n=== 主菜单 ===");
        println!("1. 扫描条形码录入商品");
        println!("2. 查看所有商品");
        println!("3. 配置 API Key (https://www.apibyte.cn/login)");
        println!("4. 退出");
        println!();
        
        let choice = read_line("请输入选项 (1-4): ")?;
        
        match choice.as_str() {
            "1" => {
                scan_barcode_flow(&db, &config).await?;
            }
            "2" => {
                list_all_products(&db)?;
            }
            "3" => {
                configure_api_key(&mut config)?;
            }
            "4" => {
                println!("\n感谢使用，再见!");
                break;
            }
            _ => {
                println!("无效的选项，请重新选择");
            }
        }
    }
    
    Ok(())
}
