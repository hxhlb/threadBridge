`codexmanager` 目前記錄的訪問端口有兩個，見 [codex_manager/README.md:25](/Volumes/Data/Github/macOSAgentBot/codex_manager/README.md#L25)：

- Web 端口：`48761`
- 服務端口：`48760`

對應 VM IP 是 `192.168.139.110`，Web 入口是：
`http://192.168.139.110:48761/`

補充一下：
`48761` 是你平常瀏覽器要訪問的端口，`48760` 是服務/API 端口；README 記錄它在未帶 API key 時會回 `missing api key`。