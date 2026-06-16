# Windowsサービスセットアップ方法

## 登録

```ps1
sc create Luwiki binPath= "\"$env:USERPROFILE\Tools\luwiki\" run --win-service"
sc config Luwiki start= auto obj= ${USER} password= ${PASSWORD}
```

## 状態確認
```ps1
sc query Luwiki
```

## 起動
```ps1
sc start Luwiki
```

## 停止
```ps1
sc stop Luwiki
```

## 削除
```ps1
sc delete Luwiki
```

