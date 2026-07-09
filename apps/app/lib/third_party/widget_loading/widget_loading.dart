import 'package:flutter/material.dart';
import 'package:loading_animation_widget/loading_animation_widget.dart';

class WidgetLoading extends StatefulWidget {
  const WidgetLoading({
    super.key,
    required this.isLoading,
    required this.child,
  });

  final bool isLoading;
  final Widget child;

  @override
  State<WidgetLoading> createState() => _WidgetLoadingState();
}

class _WidgetLoadingState extends State<WidgetLoading> {
  @override
  Widget build(BuildContext context) {
    if (!widget.isLoading) {
      return widget.child;
    }

    return Stack(
      children: [
        widget.child,
        const ModalBarrier(
          color: Colors.black38,
          dismissible: false,
        ),
        Center(
          child: LoadingAnimationWidget.discreteCircle(
            color: Colors.white,
            size: 50,
          ),
        ),
      ],
    );
  }
}
