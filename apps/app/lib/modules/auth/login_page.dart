import 'package:app/l10n/app_localizations.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:app/core/log_helper.dart';
import 'package:app/core/widgets/common_app_bar.dart';
import 'package:app/modules/app/common/service_exception.dart';
import 'package:app/modules/app/ui.dart';
import 'package:app/modules/auth/login_model.dart';

class LoginPage extends StatefulWidget {
  final Map arguments;

  const LoginPage({required this.arguments, super.key});

  @override
  State<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends State<LoginPage> {
  final LoginModel loginModel = LoginModel();

  TextEditingController nameController = TextEditingController();
  TextEditingController passwordController = TextEditingController();

  Future<void> _onLogin() async {
    try {
      var loginResult = await loginModel.exchangeToken(
          nameController.text, passwordController.text);
      if (loginResult) {
        LogHelper.info("[Login] login success");
        if (mounted) {
          UI.showToast(AppLocalizations.of(context)!.loginSuccess);
          context.pop();
        }
      } else {
        LogHelper.info("[Login] login failure");
        if (mounted) {
          UI.showError(AppLocalizations.of(context)!.loginFailure);
        }
      }
    } on ServiceException catch (e) {
      LogHelper.err(e.serviceMessage, e);
      UI.showError(e.serviceMessage);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
        appBar: CommonAppBar(
          title: Text(AppLocalizations.of(context)!.login),
          showLeading: true,
          backgroundColor: Colors.white,
          leading: BackButton(
              color: Colors.black,
              onPressed: () {
                Navigator.pop(context);
              }),
        ),
        body: Padding(
            padding: const EdgeInsets.all(10),
            child: ListView(
              children: <Widget>[
                Container(
                    alignment: Alignment.center,
                    padding: const EdgeInsets.all(10),
                    child: Text(
                      AppLocalizations.of(context)!.memo,
                      style: TextStyle(
                          color: Theme.of(context).colorScheme.primary,
                          fontWeight: FontWeight.w500,
                          fontSize: 30),
                    )),
                Container(
                  padding: const EdgeInsets.all(10),
                  child: TextField(
                    autofocus: true,
                    controller: nameController,
                    decoration: InputDecoration(
                      border: const OutlineInputBorder(),
                      labelText: AppLocalizations.of(context)!.account,
                    ),
                  ),
                ),
                Container(
                  padding: const EdgeInsets.fromLTRB(10, 10, 10, 0),
                  child: TextField(
                    obscureText: true,
                    controller: passwordController,
                    decoration: InputDecoration(
                      border: const OutlineInputBorder(),
                      labelText: AppLocalizations.of(context)!.password,
                    ),
                    onSubmitted: (value) => _onLogin(),
                  ),
                ),
                Container(
                    height: 50,
                    padding: const EdgeInsets.fromLTRB(10, 10, 10, 0),
                    child: ElevatedButton(
                      onPressed: _onLogin,
                      child: Text(AppLocalizations.of(context)!.login),
                    )),
              ],
            )));
  }
}
