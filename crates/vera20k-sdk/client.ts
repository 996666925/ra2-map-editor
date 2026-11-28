/**
 * 界面扩展入口，只转出客户端 API，不加载模拟端计时器或任务封装。
 * 界面读取请使用 Client.get；不要调用共享引用类型上的模拟端读写方法。
 */
export * from "@vera20k/sdk/client";
